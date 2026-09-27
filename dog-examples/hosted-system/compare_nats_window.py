"""Diagnostic branch only: compare a source-level write-window change on one host.

Every trial keeps the same workload, durable storage, replica count and deadline.
The source delta and binary hash are retained; this is not a released API option.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess

source = Path("dog-queue/src/backend/nats_batch.rs")
original = source.read_text()
needle = "let concurrency = if enqueue { 4 } else { 1 };"
assert original.count(needle) == 1
assert os.environ["BACKEND"] == "nats"
assert os.environ["NATS_ATOMIC"] == "on"
assert os.environ["PAYLOAD_BYTES"] == "65536"
assert os.environ["DURATION_SECONDS"] == "60"
assert os.environ["DOGRS_CAPACITY_WORKERS"] == "8"
assert os.environ["DOGRS_CAPACITY_SHARDS"] == "16"
root = Path("provider-capacity")
root.mkdir(exist_ok=True)
revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
results = []
status = 0
env = dict(os.environ, DOGRS_NATS_CONNECTIONS="per-shard",
           DOGRS_CAPACITY_COMPARISON_TENANT="dogrs-test-window-attribution")
try:
    for trial, window in enumerate([4, 8, 8, 4], 1):
        folder = root / f"trial-{trial}-window-{window}"
        folder.mkdir()
        changed = original.replace(needle, f"let concurrency = if enqueue {{ {window} }} else {{ 1 }};")
        source.write_text(changed)
        subprocess.run(["cargo", "build", "-p", "hosted-system", "--release",
                        "--features", "redis,nats", "--locked"], check=True)
        evidence = {"trial": trial, "enqueue_window": window, "metadata_window": 1,
                    "source_revision": revision, "experimental_source_delta": window != 4,
                    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                    "binary_sha256": hashlib.sha256(Path("target/release/hosted-system").read_bytes()).hexdigest(),
                    "workload": "100 tenants, 1000 offers/s, 65536 unique incompressible bytes, 60s plus 5s drain",
                    "storage": "three file replicas, sync_interval always, fresh provider per trial"}
        (folder / "experiment.json").write_text(json.dumps(evidence, indent=2))
        outcome = subprocess.run(["python3", "dog-examples/hosted-system/run_recovery.py",
                                  "nats", "--capacity", "--seconds", "60", "--bytes", "65536",
                                  "--report-dir", str(folder)], env=env)
        evidence["exit_code"] = outcome.returncode
        results.append(evidence)
        (root / "window-comparison.json").write_text(json.dumps(results, indent=2))
        if outcome.returncode:
            status = 1
finally:
    source.write_text(original)
raise SystemExit(status)
