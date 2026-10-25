# Capacity reliability investigation — 2026-10-25

Status: **not qualified at 900/s**. Do not substitute ordinary CI success for capacity evidence.

All measurements below offer 54,000 unique 64 KiB jobs across 100 tenants over 60 seconds, with the existing five-second drain and 100 ms late-offer threshold. PostgreSQL durable commits, Redis always-fsync, and NATS three file-backed always-fsync replicas remain enabled.

The local environment was the eight-vCPU/16 GiB Lima VM on an actively used Mac. Host CPU contention and approximately 98% disk utilisation were observed. Docker Desktop was stopped before the `qualification-quiet` and subsequent experiments, but this does not make the Mac a physically dedicated host. The original queue implementation is retained: longer enqueue collection windows did not establish reliability.

| Experiment / fixture | Accepted | Completed | Late offers | Overload | Errors | Passed |
|---|---:|---:|---:|---:|---:|---|
| capacity-diag/dogrs-fault-8974c5d28f | 54000 | 54000 | 127 | 0 | 0 | False |
| capacity-window-control/dogrs-fault-1b62a134a7 | 54000 | 54000 | 0 | 0 | 0 | True |
| capacity-window-candidate/dogrs-fault-ab6f21ab8b | 54000 | 54000 | 0 | 0 | 0 | True |
| capacity-window-candidate/dogrs-fault-1b1f25c245 | 54000 | 54000 | 61 | 0 | 0 | False |
| capacity-window-control/dogrs-fault-f0bbd7a886 | 54000 | 54000 | 0 | 0 | 0 | True |
| capacity20-candidate20/dogrs-fault-8c0fc6c045 | 53968 | 53968 | 2 | 32 | 0 | False |
| capacity20-control/dogrs-fault-b88fb8b52b | 54000 | 54000 | 0 | 0 | 0 | True |
| capacity20-control/dogrs-fault-1e44479966 | 35370 | 35359 | 13717 | 18402 | 501 | False |
| capacity20-candidate20/dogrs-fault-d4952a0eca | 52423 | 51272 | 762 | 1577 | 0 | False |
| qualification-quiet/round-1/dogrs-fault-4a39eb7924 | 54000 | 54000 | 0 | 0 | 0 | True |
| qualification-quiet/round-1/dogrs-fault-723051cc20 | 54000 | 54000 | 0 | 0 | 0 | True |
| qualification-quiet/round-1/dogrs-fault-c2505c48e5 | 51043 | 50981 | 4869 | 2917 | 142 | False |
| qualification-quiet/round-1/dogrs-fault-ee602b48ba | 47532 | 47482 | 6816 | 6119 | 647 | False |
| capacity-runtime-2/dogrs-fault-2752dbfbfc | 54000 | 54000 | 144 | 0 | 0 | False |
| capacity-runtime-8/dogrs-fault-c78f1c47e0 | 51054 | 51054 | 3599 | 2946 | 0 | False |
| capacity-runtime-8/dogrs-fault-9dcbdf14d2 | 50907 | 50862 | 2443 | 3093 | 0 | False |
| capacity-runtime-2/dogrs-fault-8be4b2799a | 51947 | 51947 | 2692 | 2053 | 0 | False |
| capacity-shards-8/dogrs-fault-0700140d7a | 54000 | 54000 | 27 | 0 | 0 | False |
| capacity-shards-16/dogrs-fault-5311a2cd57 | 49510 | 42147 | 103 | 3455 | 1 | False |
| capacity-shards-16/dogrs-fault-9e1bd5dd21 | 54000 | 54000 | 71 | 0 | 1 | False |
| capacity-shards-8/dogrs-fault-399ea332a2 | 39051 | 38870 | 325 | 14597 | 1 | False |

`capacity-diag` was instrumented and used a different tenant prefix; it is diagnostic evidence, not a matched capacity verdict. Other local comparisons use `dogrs-test-all-backends`. Runtime comparisons explicitly set GOMAXPROCS per NATS server; shard comparisons use fresh fixtures and never reshard existing data.

The partial `qualification-quiet` attempt was stopped after both NATS cases failed, before the remaining fourteen cases. It is a failed/incomplete qualification, not an 18-case result. All output was preserved.

The reusable qualification mode requires all eighteen measurements to pass and retains every failure. Regression checks reject reduced rate/duration and profiling. The six-case isolated ARM diagnostic with eight NATS shards is [run 40222520019](https://github.com/Jitpomi/dogrs/actions/runs/40222520019); its result must be reviewed before adopting any topology change.

Bounded late-offer samples and maximum lateness are now recorded to distinguish startup delays from mid-run stalls. They do not change scheduling, admission, timing limits, or pass/fail logic.
