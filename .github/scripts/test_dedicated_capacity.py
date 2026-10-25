"""Exercise the real dedicated-workflow loop without starting providers."""
import os
from pathlib import Path
import subprocess
import unittest


class DedicatedCapacityTests(unittest.TestCase):
    def test_both_layouts_preserve_workload_and_all_failed_results(self):
        text = (Path(__file__).resolve().parents[1] / 'workflows/capacity-baseline.yml').read_text()
        start = text.index('          status=0')
        end = text.index('      - name: Verify complete', start)
        script = '''set -e
python3() {
  printf '%s|%s|%s|%s|%s\\n' "$2" "$DOGRS_CAPACITY_WORKERS" "$DOGRS_CAPACITY_SHARDS" "$DOGRS_PG_PAYLOAD_STORAGE" "$*"
  test "$2" != nats
}
''' + text[start:end]
        for shards in ('16', '4'):
            with self.subTest(shards=shards):
                result = subprocess.run(['bash', '-c', script], capture_output=True, text=True,
                                        env=dict(os.environ, NATS_SHARDS=shards))
                self.assertEqual(result.returncode, 1, result.stderr)
                rows = [line for line in result.stdout.splitlines() if '|' in line]
                self.assertEqual(len(rows), 18)
                self.assertEqual(rows[:6], rows[6:12])
                self.assertEqual(rows[:6], rows[12:])
                for line in rows:
                    provider, workers, actual_shards, storage, args = line.split('|')
                    self.assertEqual(workers, '2' if provider == 'nats' else '1')
                    self.assertEqual(actual_shards, shards if provider == 'nats' else '1')
                    self.assertEqual(storage, 'external' if provider == 'postgres' else 'extended')
                    self.assertIn('--capacity --rate 9 --seconds 60 --bytes 65536', args)


if __name__ == '__main__':
    unittest.main()
