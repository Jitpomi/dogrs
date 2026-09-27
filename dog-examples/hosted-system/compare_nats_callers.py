"""Caller admission attribution; same backend code, offering rate and deadline."""
import os,json,hashlib,subprocess
from pathlib import Path
assert os.environ['BACKEND']=='nats'
assert os.environ['PAYLOAD_BYTES']=='65536'
assert os.environ['DURATION_SECONDS']=='60'
assert os.environ['DOGRS_CAPACITY_WORKERS']=='8'
assert os.environ['DOGRS_CAPACITY_SHARDS']=='16'
root=Path('provider-capacity');root.mkdir(exist_ok=True)
status=0;results=[]
for trial,pending in enumerate([32,64,64,32],1):
 folder=root/f'trial-{trial}-caller-window-{pending}';folder.mkdir()
 env=dict(os.environ,DOGRS_NATS_CONNECTIONS='per-shard',DOGRS_CAPACITY_INFLIGHT=str(pending),DOGRS_CAPACITY_COMPARISON_TENANT='dogrs-test-layout-attribution')
 evidence={'trial':trial,'max_pending_requests_per_tenant':pending,'framework_queue_changes':False,'source_revision':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'binary_sha256':hashlib.sha256(Path('target/release/hosted-system').read_bytes()).hexdigest(),'workload':'100 tenants, 1000 offers/s, 65536 unique incompressible bytes, 60s plus 5s drain','storage':'three file replicas, sync_interval always','scope':'caller admission buffer comparison; not a framework code fix'}
 (folder/'experiment.json').write_text(json.dumps(evidence,indent=2))
 outcome=subprocess.run(['python3','dog-examples/hosted-system/run_recovery.py','nats','--capacity','--seconds','60','--bytes','65536','--report-dir',str(folder)],env=env)
 evidence['exit_code']=outcome.returncode;results.append(evidence)
 (root/'caller-comparison.json').write_text(json.dumps(results,indent=2))
 if outcome.returncode:status=1
raise SystemExit(status)
