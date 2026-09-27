"""Diagnostic prototype only: compare raw and packed immutable payloads in the same atomic stream."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
assert os.environ["BACKEND"] == "nats"
assert os.environ["NATS_ATOMIC"] == "on"
assert os.environ["PAYLOAD_BYTES"] == "65536"
assert os.environ["DURATION_SECONDS"] == "60"
assert os.environ["DOGRS_CAPACITY_WORKERS"] == "8"
assert os.environ["DOGRS_CAPACITY_SHARDS"] == "16"
root = Path("provider-capacity")
root.mkdir(exist_ok=True)
results = []
status = 0
binary_hash = hashlib.sha256(Path("target/release/hosted-system").read_bytes()).hexdigest()
for trial, layout in enumerate(["combined", "packed-combined", "packed-combined", "combined"], 1):
    folder = root / f"trial-{trial}-{layout}"
    folder.mkdir()
    env = dict(os.environ, DOGRS_NATS_CONNECTIONS="per-shard", DOGRS_NATS_LAYOUT=layout,
               DOGRS_CAPACITY_COMPARISON_TENANT="dogrs-test-layout-attribution")
    evidence = {"trial": trial, "layout": layout, "prototype": True,
                "binary_sha256": binary_hash,
                "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
                "enqueue_window": 1 if layout == "packed-combined" else 4, "metadata_window": 1,
                "storage": "three file replicas and sync_interval always for both kinds of data",
                "workload": "100 tenants, 1000 offers/s, 65536 unique bytes, 60s plus 5s drain"}
    (folder / "experiment.json").write_text(json.dumps(evidence, indent=2))
    outcome = subprocess.run(["python3", "dog-examples/hosted-system/run_recovery.py", "nats",
                              "--capacity", "--seconds", "60", "--bytes", "65536",
                              "--report-dir", str(folder)], env=env)
    evidence["exit_code"] = outcome.returncode
    results.append(evidence)
    (root / "layout-comparison.json").write_text(json.dumps(results, indent=2))
    if outcome.returncode:
        status = 1
raise SystemExit(status)
