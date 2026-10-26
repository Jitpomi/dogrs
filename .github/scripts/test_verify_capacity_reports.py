import json
from pathlib import Path
import tempfile
import unittest

from verify_capacity_reports import verify


class QualificationEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for backend in ('postgres', 'redis', 'nats'):
            for index in range(6):
                folder = self.root / f'{backend}-{index}'
                folder.mkdir()
                environment = dict(backend=backend, queue_stage_timings=False,
                                   postgres_wait_sampling=False, cpu_sampling=False,
                                   storage_shards=16, nats_enqueue_concurrency=2,
                                   nats_update_concurrency=1, nats_atomic=True,
                                   nats_connections="shared")
                (folder / 'environment.json').write_text(json.dumps(environment))
                result = dict(tenants=100, jobs_per_second_per_tenant=9, seconds=60,
                              payload_bytes=65536, offered=54000, accepted=54000,
                              completed=54000, verified_terminal_once=54000,
                              overload=0, late_offers=0, error_count=0,
                              workers_per_tenant=2 if backend == 'nats' else 1,
                              max_inflight_per_tenant=32, passed=True, errors=[],
                              elapsed_seconds=60.5, enqueue_p95_ms=100, stage_timings=None,
                              payload_pattern='unique-per-tenant-and-sequence',
                              latency_includes_recovery=False, overload_recovery=None,
                              shards='16' if backend == 'nats' else '1')
                (folder / 'capacity.log').write_text(json.dumps(result))
                if backend == 'postgres':
                    (folder / 'durability-settings.txt').write_text(
                        'fsync | on\nfull_page_writes | on\nsynchronous_commit | on\n')
                elif backend == 'redis':
                    (folder / 'redis-durability-settings.txt').write_text(
                        'appendonly\nyes\nappendfsync\nalways\n'
                        'no-appendfsync-on-rewrite\nno\nmaxmemory-policy\nnoeviction\n')
                else:
                    for server in range(3):
                        streams = [dict(name=f'queue-{i}', config=dict(storage='file',
                                   num_replicas=3)) for i in range(server, 16, 3)]
                        (folder / f'n{server}-stream-state.json').write_text(json.dumps(
                            dict(server_id=f'server-{server}', config=dict(sync_always=True),
                                 account_details=[dict(stream_detail=streams)])))

    def test_producer_runtime_must_match_environment_and_result(self):
        for folder in self.root.iterdir():
            for name in ('environment.json', 'capacity.log'):
                path = folder / name
                data = json.loads(path.read_text())
                data['producer_runtime'] = 'isolated'
                path.write_text(json.dumps(data))
        self.assertTrue(verify(self.root)[1])
        self.assertEqual(verify(self.root, producer_runtime='isolated')[1], [])
        path = self.root / 'nats-0/capacity.log'
        data = json.loads(path.read_text())
        data['producer_runtime'] = 'shared'
        path.write_text(json.dumps(data))
        self.assertTrue(verify(self.root, producer_runtime='isolated')[1])

    def test_container_crash_or_missing_health_evidence_fails(self):
        folder = self.root / 'nats-0'
        path = folder / 'environment.json'
        env = json.loads(path.read_text())
        env['container_health_evidence'] = True
        path.write_text(json.dumps(env))
        self.assertTrue(verify(self.root)[1])
        states = {f'n{i}': dict(Running=True, OOMKilled=False, ExitCode=0) for i in range(3)}
        path = folder / 'container-health.json'
        path.write_text(json.dumps(states))
        self.assertEqual(verify(self.root)[1], [])
        for change in (dict(OOMKilled=True), dict(Running=False), dict(ExitCode=137)):
            with self.subTest(change=change):
                path.write_text(json.dumps(states | {'n0': states['n0'] | change}))
                self.assertTrue(verify(self.root)[1])

    def test_memory_budget_must_be_explicit(self):
        for folder in self.root.glob('nats-*'):
            path = folder / 'environment.json'
            env = json.loads(path.read_text())
            env['nats_gomemlimit_mib'] = 1536
            path.write_text(json.dumps(env))
        self.assertTrue(verify(self.root)[1])
        self.assertEqual(verify(self.root, nats_memory_mib=1536)[1], [])

    def test_complete_evidence_passes(self):
        counts, errors = verify(self.root)
        self.assertEqual(counts, dict(postgres=6, redis=6, nats=6))
        self.assertEqual(errors, [])

    def test_compact_layout_must_be_explicit_and_match_all_settings(self):
        for folder in self.root.glob('nats-*'):
            path = folder / 'environment.json'
            env = json.loads(path.read_text())
            env.update(storage_shards=4, nats_enqueue_concurrency=8, nats_update_concurrency=4)
            path.write_text(json.dumps(env))
            path = folder / 'capacity.log'
            result = json.loads(path.read_text())
            result['shards'] = '4'
            path.write_text(json.dumps(result))
            for path in folder.glob('*-stream-state.json'):
                state = json.loads(path.read_text())
                for account in state['account_details']:
                    account['stream_detail'] = [s for s in account['stream_detail']
                                               if int(s['name'].split('-')[-1]) < 4]
                path.write_text(json.dumps(state))
        self.assertTrue(verify(self.root)[1])
        self.assertEqual(verify(self.root, 'compact-4')[1], [])
        path = self.root / 'nats-0/environment.json'
        env = json.loads(path.read_text())
        env['nats_update_concurrency'] = 8
        path.write_text(json.dumps(env))
        self.assertTrue(verify(self.root, 'compact-4')[1])

    def test_headroom_requires_latency_and_completion_reserve(self):
        self.assertEqual(verify(self.root, headroom=True)[1], [])
        log = self.root / 'nats-0/capacity.log'
        original = json.loads(log.read_text())
        for change in [dict(elapsed_seconds=62.001), dict(enqueue_p95_ms=1000.001),
                       dict(enqueue_p95_ms=None), dict(enqueue_p95_ms=float('nan')),
                       dict(enqueue_p95_ms=True), dict(enqueue_p95_ms=-1)]:
            with self.subTest(change=change):
                log.write_text(json.dumps(original | change))
                self.assertEqual(verify(self.root)[1], [])
                self.assertTrue(verify(self.root, headroom=True)[1])
        log.write_text(json.dumps(original | dict(elapsed_seconds=62, enqueue_p95_ms=1000)))
        self.assertEqual(verify(self.root, headroom=True)[1], [])

    def test_balanced_leaders_require_explicit_policy_and_distribution(self):
        for folder in self.root.glob('nats-*'):
            path = folder / 'environment.json'
            env = json.loads(path.read_text())
            env['nats_leader_placement'] = 'balanced'
            path.write_text(json.dumps(env))
            for path in folder.glob('*-stream-state.json'):
                state = json.loads(path.read_text())
                for account in state['account_details']:
                    for stream in account['stream_detail']:
                        number = int(stream['name'].split('-')[-1])
                        stream['cluster'] = {'leader': f'node-{number % 3}'}
                path.write_text(json.dumps(state))
        self.assertTrue(verify(self.root)[1])
        self.assertEqual(verify(self.root, nats_leaders='balanced')[1], [])
        for path in (self.root / 'nats-0').glob('*-stream-state.json'):
            state = json.loads(path.read_text())
            for account in state['account_details']:
                for stream in account['stream_detail']:
                    stream['cluster']['leader'] = 'node-0'
            path.write_text(json.dumps(state))
        self.assertTrue(verify(self.root, nats_leaders='balanced')[1])

    def test_redis_rewrite_profile_requires_real_enabled_settings(self):
        for folder in self.root.glob('redis-*'):
            path = folder / 'environment.json'
            env = json.loads(path.read_text())
            env['redis_rewrite_min_mib'] = 1024
            path.write_text(json.dumps(env))
            path = folder / 'redis-durability-settings.txt'
            path.write_text(path.read_text().rstrip() + '\nauto-aof-rewrite-min-size\n1073741824\nauto-aof-rewrite-percentage\n100')
        self.assertTrue(verify(self.root)[1])
        self.assertEqual(verify(self.root, redis_rewrite_min_mib=1024)[1], [])
        path = self.root / 'redis-0/redis-durability-settings.txt'
        path.write_text(path.read_text().replace('percentage\n100', 'percentage\n0'))
        self.assertTrue(verify(self.root, redis_rewrite_min_mib=1024)[1])

    def test_fair_scheduler_requires_matching_profile(self):
        for folder in self.root.iterdir():
            path = folder / 'environment.json'
            env = json.loads(path.read_text())
            env['fair_scheduler'] = True
            path.write_text(json.dumps(env))
        self.assertTrue(verify(self.root)[1])
        self.assertEqual(verify(self.root, fair_scheduler=True)[1], [])

    def test_paced_runtime_cannot_hide_delayed_dispatch(self):
        for folder in self.root.iterdir():
            env_path = folder / 'environment.json'
            env = json.loads(env_path.read_text())
            env['producer_runtime'] = 'paced'
            env_path.write_text(json.dumps(env))
            log = folder / 'capacity.log'
            row = json.loads(log.read_text())
            row.update(producer_runtime='paced', late_dispatches=0)
            log.write_text(json.dumps(row))
        self.assertEqual(verify(self.root, producer_runtime='paced')[1], [])
        log = self.root / 'postgres-0/capacity.log'
        original = json.loads(log.read_text())
        for value in (1, None, False):
            row = original | dict(late_dispatches=value)
            log.write_text(json.dumps(row))
            self.assertTrue(verify(self.root, producer_runtime='paced')[1])
        del original['late_dispatches']
        log.write_text(json.dumps(original))
        self.assertTrue(verify(self.root, producer_runtime='paced')[1])

    def test_missing_or_duplicate_results_fail(self):
        log = self.root / 'postgres-0/capacity.log'
        log.unlink()
        self.assertTrue(verify(self.root)[1])
        log = self.root / 'redis-0/capacity.log'
        log.write_text(log.read_text() + '\n' + log.read_text())
        self.assertTrue(any('exactly one result' in error for error in verify(self.root)[1]))

    def test_green_flag_cannot_hide_loss_or_relaxed_deadline(self):
        log = self.root / 'nats-0/capacity.log'
        original = json.loads(log.read_text())
        for change in [dict(accepted=53999), dict(verified_terminal_once=53999),
                       dict(late_offers=1), dict(elapsed_seconds=66),
                       dict(elapsed_seconds=float('nan')), dict(seconds=10),
                       dict(payload_bytes=1024), dict(error_count=False),
                       dict(overload_recovery={'extra_seconds_allowed': 30})]:
            with self.subTest(change=change):
                log.write_text(json.dumps(original | change))
                self.assertTrue(verify(self.root)[1])

    def test_weakened_durability_fails_for_every_backend(self):
        pg = self.root / 'postgres-0/durability-settings.txt'
        pg.write_text(pg.read_text().replace('fsync | on', 'fsync | off'))
        redis = self.root / 'redis-0/redis-durability-settings.txt'
        redis.write_text(redis.read_text().replace('always', 'everysec'))
        nats = self.root / 'nats-0/n0-stream-state.json'
        state = json.loads(nats.read_text())
        state['account_details'][0]['stream_detail'][0]['config']['num_replicas'] = 1
        nats.write_text(json.dumps(state))
        errors = verify(self.root)[1]
        self.assertEqual(len(errors), 3)


if __name__ == '__main__':
    unittest.main()
