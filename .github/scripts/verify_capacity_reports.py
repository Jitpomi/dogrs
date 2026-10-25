"""Independently verify the complete 900/s qualification and durable fixtures."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_case(folder, backend, result):
    expected = dict(tenants=100, jobs_per_second_per_tenant=9, seconds=60,
                    payload_bytes=65536, offered=54000, accepted=54000,
                    completed=54000, verified_terminal_once=54000,
                    overload=0, late_offers=0, error_count=0,
                    workers_per_tenant=2 if backend == 'nats' else 1,
                    max_inflight_per_tenant=32)
    for key, value in expected.items():
        require(type(result.get(key)) is int and result[key] == value,
                f'{key}: expected {value}, got {result.get(key)!r}')
    require(result.get('passed') is True, 'reported gate did not pass')
    require(result.get('errors') == [], 'operation errors recorded')
    elapsed = result.get('elapsed_seconds')
    require(type(elapsed) in (float, int) and math.isfinite(elapsed)
            and 0 < elapsed <= 65, 'invalid elapsed time or drain deadline exceeded')
    require(result.get('payload_pattern') == 'unique-per-tenant-and-sequence',
            'wrong payload workload')
    require(result.get('latency_includes_recovery') is False
            and result.get('overload_recovery') is None, 'extra recovery window used')
    require(result.get('stage_timings') is None, 'profiled run is not qualification')
    require(result.get('shards') == ('16' if backend == 'nats' else '1'),
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
        require(len(servers) == 3, 'expected three distinct NATS servers')
        require(len(streams) == 16, 'expected sixteen NATS streams')


def verify(root):
    counts = Counter()
    errors = []
    for log in sorted(root.rglob('capacity.log')):
        try:
            environment = json.loads((log.parent / 'environment.json').read_text())
            backend = environment['backend']
            require(backend in ('postgres', 'redis', 'nats'), 'unknown backend')
            counts[backend] += 1
            for flag in ('queue_stage_timings', 'postgres_wait_sampling', 'cpu_sampling'):
                require(environment.get(flag) is False, f'{flag} enabled or missing')
            results = []
            for line in log.read_text().splitlines():
                if line.startswith('{'):
                    row = json.loads(line)
                    if isinstance(row, dict) and 'offered' in row:
                        results.append(row)
            require(len(results) == 1, 'expected exactly one result per fixture')
            verify_case(log.parent, backend, results[0])
        except (ValueError, OSError, KeyError, TypeError) as error:
            errors.append(f'{log.parent.name}: {error}')
    for backend in ('postgres', 'redis', 'nats'):
        if counts[backend] != 6:
            errors.append(f'{backend}: expected six measurements, found {counts[backend]}')
    return dict(counts), errors


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path)
    args = parser.parse_args()
    counts, errors = verify(args.reports)
    print(json.dumps(dict(measurements=counts, passed=not errors, errors=errors), indent=2))
    raise SystemExit(bool(errors))
