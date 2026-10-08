"""Guard rate forwarding and result validation across all diagnostic modes."""
import json
import os
import pathlib
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = pathlib.Path(__file__).resolve().parents[2] / 'dog-examples/hosted-system/compare_capacity.py'


class AttributionRateTests(unittest.TestCase):
    def run_comparison(self, requested, reported=None, failed=False):
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            binary = root / 'binary'
            binary.write_bytes(b'test-binary')
            output = root / 'results'
            calls = []

            def run(command, **kwargs):
                calls.append(command)
                self.assertEqual(command[command.index('--rate') + 1], str(requested))
                mode = command[command.index('--admission-mode') + 1] if '--admission-mode' in command else 'queue'
                rate = requested if reported is None else reported
                result = {'mode': mode, 'seconds': 10, 'tenants': 100,
                          'jobs_per_second_per_tenant': rate,
                          'offered': 10 * 100 * rate, 'accepted': 10 * 100 * rate,
                          'payload_bytes': 1024, 'production_acceptance': False,
                          'passed': not failed, 'admission_target_met': not failed}
                destination = pathlib.Path(command[command.index('--report-dir') + 1]) / 'fixture'
                destination.mkdir()
                (destination / 'capacity.log').write_text(json.dumps(result) + '\n')
                return subprocess.CompletedProcess(command, int(failed), '', '')

            args = [str(SCRIPT), '--report-dir', str(output), '--rate', str(requested),
                    '--seconds', '10', '--payloads', '1024', '--backends', 'postgres', '--repeats', '1']
            with patch.object(sys, 'argv', args), patch.dict(os.environ, {'DOGRS_SYSTEM_BINARY': str(binary)}), \
                 patch('subprocess.run', side_effect=run), \
                 patch('subprocess.check_output', side_effect=['test-head', '']), patch('builtins.print'):
                if reported is not None and reported != requested:
                    with self.assertRaisesRegex(RuntimeError, 'workload'):
                        runpy.run_path(str(SCRIPT), run_name='__main__')
                else:
                    with self.assertRaises(SystemExit) as result:
                        runpy.run_path(str(SCRIPT), run_name='__main__')
                    self.assertEqual(result.exception.code, int(failed))
                    self.assertEqual(len(calls), 4)
                    report = json.loads((output / 'comparison.json').read_text())
                    self.assertEqual(report['jobs_per_second_per_tenant'], requested)
                    self.assertEqual(len(report['runs']), 4)

    def test_selected_rates_reach_all_modes_and_validate(self):
        for rate in [5, 9, 10]:
            with self.subTest(rate=rate):
                self.run_comparison(rate)

    def test_a_different_reported_workload_is_rejected(self):
        self.run_comparison(9, reported=10)

    def test_failures_are_retained_without_stopping_later_modes(self):
        self.run_comparison(9, failed=True)


if __name__ == '__main__':
    unittest.main()
