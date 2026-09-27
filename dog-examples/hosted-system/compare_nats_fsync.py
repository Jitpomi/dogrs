#!/usr/bin/env python3
"""Diagnostic only: identical DogRS source, different provider fsync policy.

Buffered cases deliberately weaken power-loss durability and NEVER qualify a
production release. All cases are instrumented and retain the workload deadline.
"""
import hashlib
import json
import os
import pathlib
import subprocess
import sys

repo = pathlib.Path(__file__).resolve().parents[2]
root = pathlib.Path('provider-attribution').resolve()
root.mkdir(exist_ok=True)
if (root / 'fsync-comparison.json').exists():
    raise SystemExit('Choose a fresh report directory')
binary = pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve()
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
    'source_revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
    'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
    'description': 'Same binary, fresh R3 providers per case, always/2m/2m/always; buffered cases are NOT production acceptance.',
    'runs': [],
}
# Kernel counters are read-only. They include host activity and must be interpreted
# alongside the per-container counters, not attributed wholly to NATS.
sampler = '''
   def sample_host_io():
    with (folder/'host-io.jsonl').open('w') as output:
     while not profile_stop.is_set():
      row={'monotonic':time.monotonic()}
      for source in ['/proc/diskstats','/proc/stat','/proc/meminfo','/proc/vmstat','/proc/pressure/io','/proc/pressure/cpu','/proc/pressure/memory']:
       try:row[source]=pathlib.Path(source).read_text()
       except OSError as err:row[source]={'error':str(err)}
      output.write(json.dumps(row)+'\\n');output.flush()
      profile_stop.wait(1)
   host_io_thread=threading.Thread(target=sample_host_io,daemon=True);host_io_thread.start()
'''
try:
    for index, policy in enumerate(['always', '2m', '2m', 'always'], 1):
        text = original.replace('sync_interval:always', 'sync_interval:' + policy)
        text = text.replace("  'nats_storage':'anonymous Docker volume at /data',", "  'nats_storage':'anonymous Docker volume at /data',\n  'production_acceptance':False,\n  'nats_sync_interval':" + repr(policy) + ',')
        text = text.replace('   def sample_nats_stacks():', sampler + '   def sample_nats_stacks():')
        text = text.replace('    profile_stop.set()', '    profile_stop.set()\n    host_io_thread.join(timeout=5)')
        text = text.replace("'sync_policy':'always' if a.backend in ('redis','nats') else", "'sync_policy':" + repr(policy) + " if a.backend=='nats' else 'always' if a.backend=='redis' else")
        runner.write_text(text)
        folder = root / f'trial-{index}-{policy}'
        folder.mkdir()
        env = dict(os.environ, DOGRS_SYSTEM_BINARY=str(binary), DOGRS_CAPACITY_SHARDS='16',
                   DOGRS_CAPACITY_WORKERS='8', DOGRS_CAPACITY_INFLIGHT='32',
                   DOGRS_NATS_CONNECTIONS='per-shard', DOGRS_NATS_ATOMIC='1',
                   DOGRS_QUEUE_TIMINGS='1', DOGRS_CAPACITY_COMPARISON_TENANT='dogrs-test-fsync-attribution')
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
        if measurement is None or measurement.get('offered') != 60000 or measurement.get('payload_bytes') != 65536:
            raise RuntimeError('Missing or mismatched measurement')
        # Flush outstanding buffered writes before starting another independent
        # case; otherwise delayed writeback could contaminate the next control.
        subprocess.run(['sync'], check=True, timeout=120)
finally:
    runner.unlink(missing_ok=True)
    (root / 'fsync-comparison.json').write_text(json.dumps(report, indent=2) + '\n')
raise SystemExit(1 if any(row['exit_code'] for row in report['runs']) else 0)
