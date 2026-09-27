"""Isolate metadata piggybacking on one runner without changing the load gate."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

assert os.environ['BACKEND'] == 'nats'
assert os.environ['PAYLOAD_BYTES'] == '65536'
assert os.environ['DURATION_SECONDS'] == '60'
assert os.environ['DOGRS_CAPACITY_WORKERS'] == '8'
assert os.environ['DOGRS_CAPACITY_SHARDS'] == '16'
assert os.environ['NATS_ATOMIC'] == 'on'
source = Path('dog-queue/src/backend/nats_batch.rs')
original = source.read_text()
needle = 'const PIGGYBACK_METADATA: bool = true;'
assert original.count(needle) == 1
root = Path('provider-capacity')
root.mkdir(exist_ok=True)
results = []
status = 0
try:
    for trial, enabled in enumerate([False, True, True, False], 1):
        label = str(enabled).lower()
        source.write_text(original.replace(needle, f'const PIGGYBACK_METADATA: bool = {label};'))
        features = 'redis,nats'
        if os.environ.get('DOGRS_QUEUE_TIMINGS') == '1':
            features += ',queue-diagnostics'
        subprocess.run(['cargo', 'build', '-p', 'hosted-system', '--release', '--features', features, '--locked'], check=True)
        folder = root / f'trial-{trial}-metadata-{label}'
        folder.mkdir()
        env = dict(os.environ, DOGRS_NATS_CONNECTIONS='per-shard', DOGRS_CAPACITY_INFLIGHT='32', DOGRS_CAPACITY_COMPARISON_TENANT='dogrs-test-layout-attribution')
        evidence = {
            'trial': trial, 'metadata_piggyback': enabled,
            'source_revision': subprocess.check_output(['git','rev-parse','HEAD'], text=True).strip(),
            'patched_source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
            'binary_sha256': hashlib.sha256(Path('target/release/hosted-system').read_bytes()).hexdigest(),
            'workload': '100 tenants, 1000 offers/s, 65536 unique incompressible bytes, 60s plus 5s drain',
            'storage': 'three file replicas, sync_interval always',
            'queue_bounds': 'eight producer batches, one independent metadata batch, 128 messages / 2 MiB hard bound',
            'scope': 'same scheduling code in both variants; only borrowing ready metadata into producer batches differs',
        }
        (folder/'experiment.json').write_text(json.dumps(evidence, indent=2))
        result = subprocess.run(['python3','dog-examples/hosted-system/run_recovery.py','nats','--capacity','--seconds','60','--bytes','65536','--report-dir',str(folder)],env=env)
        evidence['exit_code'] = result.returncode
        results.append(evidence)
        (root/'metadata-comparison.json').write_text(json.dumps(results, indent=2))
        if result.returncode:
            status = 1
finally:
    source.write_text(original)
raise SystemExit(status)
