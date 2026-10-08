"""The shared-runner comparison must preserve settings and every failed stage."""
import os
from pathlib import Path
import subprocess
import unittest

WORKFLOW = Path(__file__).resolve().parents[1] / 'workflows/provider-capacity.yml'


class AllBackendCapacityTests(unittest.TestCase):
    def run_comparison(self, failed_provider=''):
        workflow = WORKFLOW.read_text()
        start = workflow.index('          if [ "$CAPACITY_MODE" = all-backends ]; then')
        end = workflow.index('          if [ "$CAPACITY_MODE" = queue-comparison ]; then', start)
        script = '''set -e
python3() {
  printf '%s|%s|%s|%s\\n' "$2" "$DOGRS_CAPACITY_WORKERS" "$DOGRS_CAPACITY_SHARDS" "$DOGRS_PG_PAYLOAD_STORAGE"
  test "$2" != "$FAIL_PROVIDER"
}
''' + workflow[start:end]
        env = dict(os.environ, CAPACITY_MODE='all-backends', PAYLOAD_BYTES='65536',
                   WORKER_COMPARISON='auto', NATS_ATOMIC='on', NATS_CONNECTIONS='shared',
                   PG_INSTANCES='1', PG_SHARDS='1', DOGRS_PG_FIXTURE_PARTITIONS='0',
                   PG_WAL_INIT='on', PG_PAYLOAD_STORAGE='external', PRODUCER_CAP='none',
                   CAPACITY_RATE='9', DURATION_SECONDS='60', FAIL_PROVIDER=failed_provider)
        return subprocess.run(['bash', '-c', script], env=env, capture_output=True, text=True)

    def test_preserves_backend_settings_and_reverses_order(self):
        result = self.run_comparison()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [
            'postgres|1|1|external', 'redis|1|1|extended', 'nats|2|16|extended',
            'nats|2|16|extended', 'redis|1|1|extended', 'postgres|1|1|external',
        ])

    def test_later_pass_does_not_erase_failure_or_skip_remaining_backends(self):
        result = self.run_comparison('redis')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 6)
        self.assertEqual(result.stdout.splitlines()[-1], 'postgres|1|1|external')


if __name__ == '__main__':
    unittest.main()
