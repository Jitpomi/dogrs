#!/usr/bin/env python3
"""A stalled disposable provider must produce a capacity failure report, not hang."""
import argparse
import json
import os
import pathlib
import selectors
import subprocess
import uuid

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--report-dir', required=True)
args = parser.parse_args()
name = 'dogrs-fault-deadline-' + uuid.uuid4().hex[:10]
folder = pathlib.Path(args.report_dir).resolve() / name
folder.mkdir(parents=True)
binary = str(pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve())
proc = None
output = ''
try:
    subprocess.run(['docker', 'run', '-d', '--name', name,
                    '-p', '127.0.0.1::6379', 'redis:7.4-alpine',
                    '--save', '', '--appendonly', 'yes', '--appendfsync', 'always'],
                   check=True, stdout=subprocess.DEVNULL)
    port = subprocess.check_output(['docker', 'port', name, '6379'], text=True).strip().rsplit(':', 1)[1]
    env = dict(os.environ, DOGRS_BACKEND='redis', DOGRS_REDIS_URL=f'redis://127.0.0.1:{port}',
               DOGRS_TEST_TENANT='dogrs-test-deadline', DOGRS_CAPACITY_TENANTS='1',
               DOGRS_CAPACITY_SECONDS='1', DOGRS_CAPACITY_BYTES='1024')
    proc = subprocess.Popen([binary, 'capacity-local'], env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, bufsize=1)
    with selectors.DefaultSelector() as ready:
        ready.register(proc.stdout, selectors.EVENT_READ)
        while True:
            if not ready.select(15):
                raise RuntimeError('capacity fixture did not start')
            line = proc.stdout.readline()
            output += line
            if 'phase=offering' in line:
                break
            if not line:
                raise RuntimeError('capacity fixture exited before offering')
    subprocess.run(['docker', 'pause', name], check=True, stdout=subprocess.DEVNULL)
    # Allow the separate bounded verification stage if any offer beat the pause.
    tail, _ = proc.communicate(timeout=45)
    output += tail
    result = next(json.loads(line) for line in output.splitlines() if line.startswith('{'))
    assert proc.returncode != 0 and result['passed'] is False
    assert any('submission deadline expired' in error or 'verification timed out' in error
               for error in result['errors']), result['errors']
    evidence = {'fixture': 'capacity-deadline', 'expected_failure_observed': True,
                'accepted': result['accepted'], 'errors': result['errors']}
    (folder / 'deadline-verification.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence))
finally:
    if proc is not None and proc.poll() is None:
        proc.kill()
        tail, _ = proc.communicate()
        output += tail
    (folder / 'deadline.log').write_text(output)
    subprocess.run(['docker', 'unpause', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(['docker', 'rm', '-fv', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
