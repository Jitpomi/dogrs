#!/usr/bin/env python3
"""Diagnostic only: identical DogRS source, different provider fsync policy.

Buffered cases deliberately weaken power-loss durability and NEVER qualify a
production release. All cases retain the workload deadline; queue/stack profiling is optional.
"""
import hashlib
import json
import os
import pathlib
import subprocess
import sys
from diagnostic_result import validate_result

repo = pathlib.Path(__file__).resolve().parents[2]
root = pathlib.Path('provider-attribution').resolve()
root.mkdir(exist_ok=True)
if (root / 'fsync-comparison.json').exists():
    raise SystemExit('Choose a fresh report directory')
binary = pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve()
profile = os.environ.get('DOGRS_FSYNC_PROFILE', '1')
if profile not in ('0', '1'):
    raise SystemExit('DOGRS_FSYNC_PROFILE must be 0 or 1')
original = (repo / 'dog-examples/hosted-system/run_recovery.py').read_text()
runner = repo / 'dog-examples/hosted-system/_fsync_diagnostic_runner.py'
assert not runner.exists()
assert original.count('sync_interval:always') == 1
hardware = {}
for key, command in {
    'block_devices': ['lsblk', '--json', '-o', 'NAME,TYPE,SIZE,MOUNTPOINTS,PKNAME'],
    'filesystems': ['df', '-hT'],
    'mounts': ['findmnt', '--json'],
    'docker_storage': ['docker', 'info', '--format', '{{json .}}'],
}.items():
    result = subprocess.run(command, capture_output=True, text=True, timeout=20)
    hardware[key] = {'returncode': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr}
(root / 'hardware.json').write_text(json.dumps(hardware, indent=2))
report = {
    'production_acceptance': False,
    'experiment_completed': False,
    'queue_and_stack_profiling': profile == '1',
    'source_revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
    'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
    'description': 'Same binary, fresh R3 providers per case, always/2m/2m/always; buffered cases are NOT production acceptance.',
    'runs': [],
}
try:
    for index, policy in enumerate(['always', '2m', '2m', 'always'], 1):
        text = original.replace('sync_interval:always', 'sync_interval:' + policy)
        text = text.replace("  'nats_storage':'anonymous Docker volume at /data',", "  'nats_storage':'anonymous Docker volume at /data',\n  'production_acceptance':False,\n  'nats_sync_interval':" + repr(policy) + ',')
        text = text.replace("'sync_policy':'always' if a.backend in ('redis','nats') else", "'sync_policy':" + repr(policy) + " if a.backend=='nats' else 'always' if a.backend=='redis' else")
        runner.write_text(text)
        folder = root / f'trial-{index}-{policy}'
        folder.mkdir()
        env = dict(os.environ, DOGRS_SYSTEM_BINARY=str(binary), DOGRS_CAPACITY_SHARDS='16',
                   DOGRS_CAPACITY_WORKERS='8', DOGRS_CAPACITY_INFLIGHT='32',
                   DOGRS_NATS_CONNECTIONS='per-shard', DOGRS_NATS_ATOMIC='1',
                   DOGRS_QUEUE_TIMINGS=profile, DOGRS_HOST_IO_PROFILE='1', DOGRS_CAPACITY_COMPARISON_TENANT='dogrs-test-fsync-attribution')
        for key in ['DOGRS_ADMISSION_MODE', 'DOGRS_PERF']:
            env.pop(key, None)
        result = subprocess.run([sys.executable, str(runner), 'nats', '--capacity', '--seconds', '60',
                                 '--bytes', '65536', '--report-dir', str(folder)],
                                env=env, cwd=repo, capture_output=True, text=True, timeout=420)
        (folder / 'controller.stdout').write_text(result.stdout)
        (folder / 'controller.stderr').write_text(result.stderr)
        measurement = None
        for log in folder.glob('*/capacity.log'):
            for line in log.read_text().splitlines():
                if line.startswith('{'):
                    value = json.loads(line)
                    if 'accepted' in value:
                        measurement = value
        row = {'trial': index, 'sync_interval': policy, 'production_acceptance': False,
               'exit_code': result.returncode, 'measurement': measurement}
        report['runs'].append(row)
        (root / 'fsync-comparison.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'trial': index, 'sync_interval': policy, 'exit_code': result.returncode,
                          'accepted': measurement.get('accepted') if measurement else None,
                          'completed': measurement.get('completed') if measurement else None}), flush=True)
        row['workload_gate_met'] = validate_result(measurement, result.returncode)
        if not row['workload_gate_met']:
            print(f'::warning::Trial {index} ({policy}) completed with a capacity miss; see the measured counters. This is not a production pass.', flush=True)
        # Flush outstanding buffered writes before starting another independent
        # case; otherwise delayed writeback could contaminate the next control.
        subprocess.run(['sync'], check=True, timeout=120)
    report['experiment_completed'] = True
finally:
    runner.unlink(missing_ok=True)
    (root / 'fsync-comparison.json').write_text(json.dumps(report, indent=2) + '\n')
# Reaching here means all four experiments completed and passed validation.
# Capacity misses remain explicit in the report; setup, runtime, integrity,
# incomplete-report and unexpected process failures still exit unsuccessfully.
