WITH stamped AS MATERIALIZED (SELECT clock_timestamp() AS now)
INSERT INTO dogrs_queue_jobs_v2
 (tenant,id,state,queue,kind,dedupe,priority,created_at,updated_at,eligible_at,lease_until,status,active,payload)
SELECT $1,$2,
 jsonb_set(jsonb_set($3::jsonb,'{record,created_at}',to_jsonb(stamped.now)),
           '{record,updated_at}',to_jsonb(stamped.now)),
 $4,$5,$6,$7,stamped.now,stamped.now,$8,NULL,'enqueued',true,$9
FROM stamped
ON CONFLICT (tenant,queue,kind,dedupe) WHERE active AND dedupe IS NOT NULL
DO UPDATE SET id=dogrs_queue_jobs_v2.id
RETURNING id
