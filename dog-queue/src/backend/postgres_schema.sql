CREATE TABLE IF NOT EXISTS dogrs_queue_jobs_v2 (
    tenant TEXT NOT NULL, id TEXT NOT NULL, state JSONB NOT NULL, payload BYTEA,
    queue TEXT NOT NULL, kind TEXT NOT NULL, dedupe TEXT,
    priority INTEGER NOT NULL, created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL, eligible_at TIMESTAMPTZ NOT NULL,
    lease_until TIMESTAMPTZ, status TEXT NOT NULL, active BOOLEAN NOT NULL,
    PRIMARY KEY (tenant,id)
);
CREATE UNIQUE INDEX IF NOT EXISTS dogrs_queue_dedupe_v2
    ON dogrs_queue_jobs_v2(tenant,queue,kind,dedupe) WHERE active AND dedupe IS NOT NULL;
CREATE INDEX IF NOT EXISTS dogrs_queue_claim_v2
    ON dogrs_queue_jobs_v2(tenant,queue,priority DESC,created_at,id)
    INCLUDE (eligible_at) WHERE status IN ('enqueued','retrying');
CREATE INDEX IF NOT EXISTS dogrs_queue_expired_v2
    ON dogrs_queue_jobs_v2(lease_until,tenant) WHERE lease_until IS NOT NULL;
CREATE INDEX IF NOT EXISTS dogrs_queue_retention_v2
    ON dogrs_queue_jobs_v2(tenant,updated_at) WHERE NOT active;
CREATE TABLE IF NOT EXISTS dogrs_queue_metadata_v2(version INTEGER PRIMARY KEY);
CREATE TABLE IF NOT EXISTS dogrs_queue_state_v1(tenant TEXT PRIMARY KEY,state JSONB NOT NULL);

CREATE INDEX IF NOT EXISTS dogrs_queue_runnable_v2
    ON dogrs_queue_jobs_v2(tenant,queue,eligible_at)
    WHERE status IN ('enqueued','retrying');

ALTER TABLE dogrs_queue_jobs_v2 ADD COLUMN IF NOT EXISTS payload BYTEA;
