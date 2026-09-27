WITH stamped AS MATERIALIZED (
 SELECT clock_timestamp() AS now,
        $8::timestamptz = '1970-01-01 00:00:00+00'::timestamptz AS immediate
)
INSERT INTO dogrs_queue_jobs_v2
 (tenant,id,state,queue,kind,dedupe,priority,created_at,updated_at,eligible_at,lease_until,status,active,payload)
SELECT $1,$2,
 jsonb_set(jsonb_set(CASE WHEN stamped.immediate
   THEN jsonb_set($3::jsonb,'{record,message,run_at}',to_jsonb(stamped.now))
   ELSE $3::jsonb END,'{record,created_at}',to_jsonb(stamped.now)),
           '{record,updated_at}',to_jsonb(stamped.now)),
 $4,$5,$6,$7,stamped.now,stamped.now,
 CASE WHEN stamped.immediate THEN stamped.now ELSE $8 END,NULL,'enqueued',true,$9
FROM stamped
ON CONFLICT (tenant,queue,kind,dedupe) WHERE active AND dedupe IS NOT NULL
DO UPDATE SET id=dogrs_queue_jobs_v2.id
RETURNING id
