"""Pipeline comparison must isolate one limit and retain every failed trial."""
import os
from pathlib import Path
import subprocess
import unittest

WORKFLOW = Path(__file__).resolve().parents[1] / 'workflows/provider-capacity.yml'


class NatsPipelineCapacityTests(unittest.TestCase):
    def comparison(self, fail_slots=''):
        workflow = WORKFLOW.read_text()
        start = workflow.index('          if [ "$CAPACITY_MODE" = nats-pipeline-comparison ]; then')
        end = workflow.index('          if [ "$WORKER_COMPARISON" = compare ]; then', start)
        script = '''set -e
python3() {
  printf '%s|%s|%s|%s\\n' "$2" "$DOGRS_CAPACITY_WORKERS" "$DOGRS_NATS_ENQUEUE_CONCURRENCY" "$DOGRS_NATS_UPDATE_CONCURRENCY"
  test "$DOGRS_NATS_ENQUEUE_CONCURRENCY" != "$FAIL_SLOTS"
}
''' + workflow[start:end]
        env = dict(os.environ, CAPACITY_MODE='nats-pipeline-comparison', BACKEND='nats',
                   PAYLOAD_BYTES='65536', NATS_ATOMIC='on', NATS_CONNECTIONS='shared',
                   DOGRS_CAPACITY_SHARDS='16', CAPACITY_RATE='9', DURATION_SECONDS='60',
                   FAIL_SLOTS=fail_slots)
        return subprocess.run(['bash', '-c', script], env=env, capture_output=True, text=True)

    def test_fixed_consumers_and_metadata_with_reversed_producer_limits(self):
        result = self.comparison()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ['nats|8|2|1', 'nats|8|4|1',
                                                     'nats|8|4|1', 'nats|8|2|1'])

    def test_preserves_candidate_failure_and_runs_final_control(self):
        result = self.comparison('4')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 4)
        self.assertEqual(result.stdout.splitlines()[-1], 'nats|8|2|1')


if __name__ == '__main__':
    unittest.main()
