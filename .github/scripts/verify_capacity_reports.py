"""Independently verify the complete 900/s qualification and durable fixtures."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


NATS_LAYOUTS = {"original-16": (16, 2, 1), "compact-4": (4, 8, 4)}


def verify_case(folder, backend, result, nats_layout="original-16", headroom=False, nats_leaders="natural"):
    nats_shards, _, _ = NATS_LAYOUTS[nats_layout]
    expected = dict(tenants=100, jobs_per_second_per_tenant=9, seconds=60,
                    payload_bytes=65536, offered=54000, accepted=54000,
                    completed=54000, verified_terminal_once=54000,
                    overload=0, late_offers=0, error_count=0,
                    workers_per_tenant=2 if backend == 'nats' else 1,
                    max_inflight_per_tenant=32)
    for key, value in expected.items():
        require(type(result.get(key)) is int and result[key] == value,
                f'{key}: expected {value}, got {result.get(key)!r}')
    if result.get('producer_runtime') == 'paced':
        require(type(result.get('late_dispatches')) is int and result['late_dispatches'] == 0,
                'late or missing enqueue dispatch evidence')
    if 'verification_concurrency' in result:
        require(type(result['verification_concurrency']) is int
                and result['verification_concurrency'] == 4
                and type(result.get('verification_budget_seconds')) is int
                and result['verification_budget_seconds'] == 30
                and result.get('verification_identity_checked') is True,
                'snapshot verification bounds or identity checks changed')
    require(result.get('passed') is True, 'reported gate did not pass')
    require(result.get('errors') == [], 'operation errors recorded')
    elapsed = result.get('elapsed_seconds')
    require(type(elapsed) in (float, int) and math.isfinite(elapsed)
            and 0 < elapsed <= 65, 'invalid elapsed time or drain deadline exceeded')
    if headroom:
        require(elapsed <= 62, 'headroom: completion exceeded 62 seconds')
        latency = result.get('enqueue_p95_ms')
        require(type(latency) in (float, int) and math.isfinite(latency)
                and 0 <= latency <= 1000, 'headroom: enqueue p95 exceeded 1000 ms or missing')
    require(result.get('payload_pattern') == 'unique-per-tenant-and-sequence',
            'wrong payload workload')
    require(result.get('latency_includes_recovery') is False
            and result.get('overload_recovery') is None, 'extra recovery window used')
    require(result.get('stage_timings') is None, 'profiled run is not qualification')
    require(result.get('shards') == (str(nats_shards) if backend == 'nats' else '1'),
            'unexpected storage topology')
    if backend == 'postgres':
        settings = {}
        for line in (folder / 'durability-settings.txt').read_text().splitlines():
            if '|' in line:
                key, value = line.split('|', 1)
                settings[key.strip()] = value.strip()
        for key in ('fsync', 'full_page_writes', 'synchronous_commit'):
            require(settings.get(key) == 'on', f'PostgreSQL {key} is not on')
    elif backend == 'redis':
        lines = (folder / 'redis-durability-settings.txt').read_text().splitlines()
        require(len(lines) % 2 == 0, 'incomplete Redis settings')
        settings = dict(zip(lines[::2], lines[1::2]))
        for key, value in [('appendonly', 'yes'), ('appendfsync', 'always'),
                           ('no-appendfsync-on-rewrite', 'no'),
                           ('maxmemory-policy', 'noeviction')]:
            require(settings.get(key) == value, f'Redis {key} is not {value}')
    elif backend == 'nats':
        snapshots = list(folder.glob('*-stream-state.json'))
        require(len(snapshots) == 3, 'expected three NATS server snapshots')
        streams = set()
        leaders = {}
        servers = set()
        for snapshot in snapshots:
            state = json.loads(snapshot.read_text())
            require(isinstance(state.get('server_id'), str) and state['server_id'],
                    'NATS server identity missing')
            servers.add(state['server_id'])
            require(state.get('config', {}).get('sync_always') is True,
                    'NATS always-fsync is not enabled')
            for account in state.get('account_details', []):
                for stream in account.get('stream_detail', []):
                    config = stream.get('config', {})
                    require(config.get('storage') == 'file'
                            and config.get('num_replicas') == 3,
                            'NATS stream is not file-backed with three replicas')
                    streams.add(stream['name'])
                    if nats_leaders == 'balanced':
                        leader = stream.get('cluster', {}).get('leader')
                        require(isinstance(leader, str) and leader, 'NATS leader missing')
                        require(stream['name'] not in leaders or leaders[stream['name']] == leader,
                                'NATS snapshots disagree on leader')
                        leaders[stream['name']] = leader
        require(len(servers) == 3, 'expected three distinct NATS servers')
        require(len(streams) == nats_shards, f'expected {nats_shards} NATS streams')
        if nats_leaders == 'balanced':
            loads = Counter(leaders.values())
            require(len(loads) == 3 and max(loads.values()) - min(loads.values()) <= 1,
                    'NATS leaders are not balanced across three servers')


def verify(root, nats_layout="original-16", headroom=False, nats_leaders="natural", producer_runtime="shared", nats_memory_mib=0, fair_scheduler=False, redis_rewrite_min_mib=64):
    require(nats_layout in NATS_LAYOUTS, "unknown NATS layout")
    require(nats_leaders in ('natural', 'balanced'), 'unknown leader policy')
    require(producer_runtime in ("shared", "isolated", "paced"), "unknown producer runtime")
    counts = Counter()
    errors = []
    for log in sorted(root.rglob('capacity.log')):
        try:
            environment = json.loads((log.parent / 'environment.json').read_text())
            require(environment.get('producer_runtime', 'shared') == producer_runtime,
                    'unexpected producer runtime in environment')
            require(environment.get('fair_scheduler', False) is fair_scheduler,
                    'unexpected runtime scheduling profile')
            backend = environment['backend']
            require(backend in ('postgres', 'redis', 'nats'), 'unknown backend')
            counts[backend] += 1
            if environment.get('container_health_evidence') is True:
                states = json.loads((log.parent / 'container-health.json').read_text())
                require(len(states) == (3 if backend == 'nats' else 1),
                        'missing container health evidence')
                for state in states.values():
                    require(state.get('Running') is True and state.get('OOMKilled') is False
                            and state.get('ExitCode') == 0, 'server crashed or was OOM killed')
            for flag in ('queue_stage_timings', 'postgres_wait_sampling', 'cpu_sampling'):
                require(environment.get(flag) is False, f'{flag} enabled or missing')
            if backend == 'redis':
                require(environment.get('redis_rewrite_min_mib', 64) == redis_rewrite_min_mib,
                        'unexpected Redis rewrite profile')
                values = (log.parent / 'redis-durability-settings.txt').read_text().splitlines()
                settings = dict(zip(values[::2], values[1::2]))
                if 'redis_rewrite_min_mib' in environment:
                    require(settings.get('auto-aof-rewrite-min-size') == str(redis_rewrite_min_mib * 1024 * 1024)
                            and settings.get('auto-aof-rewrite-percentage') == '100',
                            'Redis rewrite settings differ or automatic rewrites disabled')
            if backend == 'nats':
                require(environment.get('nats_gomemlimit_mib', 0) == nats_memory_mib,
                        'unexpected NATS Go memory budget')
                require(environment.get('nats_leader_placement', 'natural') == nats_leaders,
                        'unexpected leader policy')
                shards, enqueues, updates = NATS_LAYOUTS[nats_layout]
                for key, expected in [('storage_shards', shards),
                                      ('nats_enqueue_concurrency', enqueues),
                                      ('nats_update_concurrency', updates)]:
                    require(type(environment.get(key)) is int and environment[key] == expected,
                            f'{key}: expected {expected}')
                require(environment.get('nats_atomic') is True, 'atomic batching disabled')
                require(environment.get('nats_connections') == 'shared', 'unexpected connections')
            results = []
            for line in log.read_text().splitlines():
                if line.startswith('{'):
                    row = json.loads(line)
                    if isinstance(row, dict) and 'offered' in row:
                        results.append(row)
            require(len(results) == 1, 'expected exactly one result per fixture')
            require(results[0].get('producer_runtime', 'shared') == producer_runtime,
                    'unexpected producer runtime in result')
            verify_case(log.parent, backend, results[0], nats_layout, headroom, nats_leaders)
        except (ValueError, OSError, KeyError, TypeError) as error:
            errors.append(f'{log.parent.name}: {error}')
    for backend in ('postgres', 'redis', 'nats'):
        if counts[backend] != 6:
            errors.append(f'{backend}: expected six measurements, found {counts[backend]}')
    return dict(counts), errors


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path)
    parser.add_argument('--nats-layout', choices=NATS_LAYOUTS, default='original-16')
    parser.add_argument('--headroom', action='store_true',
                        help='Also require completion <=62s and enqueue p95 <=1000ms in every case')
    parser.add_argument('--nats-leaders', choices=['natural', 'balanced'], default='natural')
    parser.add_argument('--producer-runtime', choices=['shared', 'isolated', 'paced'], default='shared')
    parser.add_argument('--nats-memory-mib', type=int, default=0)
    parser.add_argument('--fair-scheduler', action='store_true')
    parser.add_argument('--redis-rewrite-min-mib', type=int, default=64)
    args = parser.parse_args()
    counts, errors = verify(args.reports, args.nats_layout, args.headroom, args.nats_leaders, args.producer_runtime, args.nats_memory_mib, args.fair_scheduler, args.redis_rewrite_min_mib)
    print(json.dumps(dict(measurements=counts, passed=not errors, errors=errors), indent=2))
    raise SystemExit(bool(errors))
