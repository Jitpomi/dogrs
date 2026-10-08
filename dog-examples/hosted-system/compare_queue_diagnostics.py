#!/usr/bin/env python3
"""Compare diagnostics off/on/on/off on one runner; preserve every failure."""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('backend', choices=['redis', 'nats'])
parser.add_argument('--report-dir', required=True)
parser.add_argument('--rate', type=int, choices=range(1, 11), default=9)
parser.add_argument('--seconds', type=int, choices=[60, 120], default=60)
args = parser.parse_args()
root = pathlib.Path(args.report_dir).resolve()
root.mkdir(parents=True, exist_ok=True)
report_path = root / 'diagnostic-comparison.json'
if report_path.exists():
    parser.error('report directory already contains a comparison')
binary = pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve()
report = {'backend': args.backend, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
          'order': [False, True, True, False], 'runs': [], 'passed': False}
for trial, enabled in enumerate(report['order'], 1):
    folder = root / f'trial-{trial}'
    folder.mkdir(exist_ok=False)
    env = dict(os.environ, DOGRS_QUEUE_TIMINGS='1' if enabled else '0',
               DOGRS_CAPACITY_COMPARISON_TENANT='dogrs-test-diagnostic-comparison')
    with (folder / 'controller.log').open('w') as output:
        result = subprocess.run(['python3', str(pathlib.Path(__file__).with_name('run_recovery.py')),
                                 args.backend, '--capacity', '--rate', str(args.rate),
                                 '--seconds', str(args.seconds), '--bytes', '65536',
                                 '--report-dir', str(folder)], env=env,
                                stdout=output, stderr=subprocess.STDOUT)
    measurements = []
    for log in folder.rglob('capacity.log'):
        for line in log.read_text().splitlines():
            if line.startswith('{'):
                value = json.loads(line)
                if 'offered' in value:
                    measurements.append(value)
    row = {'trial': trial, 'diagnostics': enabled, 'exit_code': result.returncode,
           'measurements': measurements}
    report['runs'].append(row)
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(row), flush=True)
report['passed'] = all(r['exit_code'] == 0 and len(r['measurements']) == 1
                       and r['measurements'][0].get('passed') is True for r in report['runs'])
report_path.write_text(json.dumps(report, indent=2) + '\n')
raise SystemExit(0 if report['passed'] else 1)
