#!/usr/bin/env python3
"""Serial, fresh-server admission comparisons. These diagnostics do not certify production.

The PostgreSQL native-layout baseline intentionally shares DogRS's schema/INSERT
SQL, so a matching result isolates the Rust path but does NOT exonerate that SQL.
All fixtures retain their ordinary fsync/replication settings.
"""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--report-dir', required=True)
parser.add_argument('--rate', type=int, choices=range(1, 11), default=9)
parser.add_argument('--seconds', type=int, choices=[10, 30, 60], default=10)
parser.add_argument('--backends', nargs='+', choices=['postgres', 'nats'], default=['postgres', 'nats'])
parser.add_argument('--payloads', nargs='+', type=int, choices=[1024, 65536], default=[1024, 65536])
parser.add_argument('--repeats', type=int, choices=[1, 2, 3], default=2)
args = parser.parse_args()
repo = pathlib.Path(__file__).resolve().parents[2]
root = pathlib.Path(args.report_dir).resolve()
root.mkdir(parents=True, exist_ok=True)
if (root / 'comparison.json').exists():
    parser.error('choose a fresh report directory to preserve prior evidence')
binary = pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve()
env = dict(os.environ, DOGRS_SYSTEM_BINARY=str(binary), DOGRS_NATS_IMAGE='nats:2.15.0-alpine',
           DOGRS_CAPACITY_INFLIGHT='32', DOGRS_CAPACITY_COMPARISON_TENANT='dogrs-test-comparison', DOGRS_PG_POOL_SIZE='64', DOGRS_PG_COMMIT_DELAY='0', DOGRS_PG_ENQUEUE_BATCH_SIZE='1')
for key in ['DOGRS_ADMISSION_MODE', 'DOGRS_PG_ENQUEUE_CONCURRENCY', 'DOGRS_NATS_ATOMIC_ENQUEUE']:
    env.pop(key, None)
digest = hashlib.sha256()
with binary.open('rb') as source:
    for block in iter(lambda: source.read(1024 * 1024), b''):
        digest.update(block)
report = {'production_acceptance': False, 'scope': 'admission diagnostics plus separately labelled full-queue measurements',
          'seconds': args.seconds, 'jobs_per_second_per_tenant': args.rate, 'repeats': args.repeats,
          'git_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
          'binary_sha256': digest.hexdigest(),
          'source_dirty': bool(subprocess.check_output(['git','status','--porcelain'],cwd=repo,text=True)),
          'notes': ['Fresh provider containers per measurement; runs are serial.',
                    'Odd repeats reverse mode order to expose order/host variance.',
                    'Native-layout shares PostgreSQL schema and single-row admission SQL; PostgreSQL batching is disabled in every mode.',
                    'This comparison cannot rule out SQL/layout costs or certify the batched configuration.',
                    'Native admission omits discovery, claims, completions, recovery and duplicate-submission races.',
                    'Short-run results do not certify the sustained production target.'], 'runs': []}
try:
    for repeat in range(args.repeats):
        for payload in args.payloads:
            for backend in args.backends:
                env.update(DOGRS_CAPACITY_SHARDS='16' if backend == 'nats' else '1',
                           DOGRS_CAPACITY_WORKERS='2' if backend == 'nats' else '1')
                modes = ['native-payload', 'native-layout', 'dogrs-admission', 'queue']
                if repeat % 2:
                    modes.reverse()
                for mode in modes:
                    folder = root / f'{repeat}-{backend}-{payload}-{mode}'
                    folder.mkdir(parents=True)
                    command = ['python3', 'dog-examples/hosted-system/run_recovery.py', backend,
                               '--capacity', '--rate', str(args.rate), '--seconds', str(args.seconds), '--bytes', str(payload), '--report-dir', str(folder)]
                    if mode != 'queue':
                        command += ['--admission-mode', mode]
                    outcome = subprocess.run(command, cwd=repo, env=env, capture_output=True, text=True, timeout=420)
                    (folder / 'controller.stdout').write_text(outcome.stdout)
                    (folder / 'controller.stderr').write_text(outcome.stderr)
                    result = None
                    for log in folder.glob('*/capacity.log'):
                        for line in log.read_text().splitlines():
                            try:
                                value = json.loads(line)
                            except ValueError:
                                continue
                            if isinstance(value, dict) and 'accepted' in value:
                                result = value
                    row = {'repeat': repeat, 'backend': backend, 'payload_bytes': payload,
                           'mode': mode, 'exit_code': outcome.returncode, 'result': result}
                    report['runs'].append(row)
                    (root / 'comparison.json').write_text(json.dumps(report, indent=2) + '\n')
                    print(json.dumps({k: row[k] for k in ('repeat', 'backend', 'payload_bytes', 'mode', 'exit_code')} |
                                     {'accepted': result.get('accepted') if result else None,
                                      'completed': result.get('completed') if result else None}), flush=True)
                    if result is None:
                        raise RuntimeError('measurement absent; inspect controller/setup logs')
                    if result.get('offered') != args.seconds * 1000 or result.get('payload_bytes') != payload:
                        raise RuntimeError('measurement does not match requested workload')
                    if mode != 'queue' and (result.get('mode') != mode or result.get('production_acceptance') is not False):
                        raise RuntimeError('diagnostic scope mismatch')
finally:
    (root / 'comparison.json').write_text(json.dumps(report, indent=2) + '\n')
# An unsuccessful stage remains visible to CI; comparisons are never a waiver.
raise SystemExit(1 if any(r['exit_code'] for r in report['runs']) else 0)
