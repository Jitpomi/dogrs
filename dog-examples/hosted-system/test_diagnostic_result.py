import unittest
from diagnostic_result import validate_result


def measurement(**changes):
    result = dict(offered=60000, accepted=58000, completed=58000,
                  verified_terminal_once=58000, overload=2000, late_offers=0,
                  error_count=0, errors=[], passed=False, payload_bytes=65536,
                  tenants=100, seconds=60, jobs_per_second_per_tenant=10,
                  workers_per_tenant=8, max_inflight_per_tenant=32, shards='16',
                  nats_connections='per-shard', payload_pattern='unique-per-tenant-and-sequence',
                  latency_includes_recovery=False, overload_recovery=None, elapsed_seconds=61.0)
    result.update(changes)
    return result


class DiagnosticResultTests(unittest.TestCase):
    def test_capacity_miss_is_a_complete_experiment(self):
        self.assertFalse(validate_result(measurement(), 1))
        self.assertFalse(validate_result(measurement(accepted=60000, completed=60000,
                         verified_terminal_once=60000, overload=0, late_offers=1), 1))
        self.assertFalse(validate_result(measurement(accepted=60000, completed=60000,
                         verified_terminal_once=60000, overload=0, elapsed_seconds=65.1), 1))

    def test_workload_pass_is_reported_separately(self):
        self.assertTrue(validate_result(measurement(accepted=60000, completed=60000,
                        verified_terminal_once=60000, overload=0, passed=True), 0))

    def test_errors_and_missing_or_inconsistent_evidence_fail(self):
        bad = [None, {}, measurement(error_count=1, errors=['timeout']),
               measurement(errors=['corrupt payload']), measurement(completed=57999),
               measurement(verified_terminal_once=57999), measurement(overload=1999),
               measurement(passed=True), measurement(passed='false'),
               measurement(accepted=True), measurement(late_offers=-1),
               measurement(late_offers=60001), measurement(payload_bytes=1024),
               measurement(seconds=10), measurement(shards='1'),
               measurement(payload_pattern='reused'), measurement(elapsed_seconds=float('nan')),
               measurement(latency_includes_recovery=True),
               measurement(overload_recovery={'all_acknowledged_jobs_completed_once': True})]
        for row in bad:
            with self.subTest(row=row):
                with self.assertRaises(ValueError):
                    validate_result(row, 1)

    def test_unexpected_exit_status_is_not_hidden(self):
        for code in (0, 2, 137, -9):
            with self.subTest(code=code):
                with self.assertRaises(ValueError):
                    validate_result(measurement(), code)


if __name__ == '__main__':
    unittest.main()
