WITH input AS MATERIALIZED (
 SELECT * FROM unnest($1::text[],$2::text[],$3::jsonb[],$4::text[],
  $5::text[],$6::text[],$7::int[],$8::timestamptz[],$9::bytea[])
 WITH ORDINALITY AS i(tenant,id,state,queue,kind,dedupe,priority,run_at,payload,ordinal)
), stamped AS MATERIALIZED (
 SELECT *,clock_timestamp() AS now,
  run_at='1970-01-01 00:00:00+00'::timestamptz AS immediate FROM input
), written AS (
 INSERT INTO dogrs_queue_jobs_v2
  (tenant,id,state,queue,kind,dedupe,priority,created_at,updated_at,eligible_at,lease_until,status,active,payload)
 SELECT tenant,id,
  jsonb_set(jsonb_set(CASE WHEN immediate
    THEN jsonb_set(state,'{record,message,run_at}',to_jsonb(now)) ELSE state END,
    '{record,created_at}',to_jsonb(now)), '{record,updated_at}',to_jsonb(now)),
  queue,kind,dedupe,priority,now,now,CASE WHEN immediate THEN now ELSE run_at END,
  NULL,'enqueued',true,payload
 FROM stamped ORDER BY tenant,queue,kind,dedupe,id
 ON CONFLICT (tenant,queue,kind,dedupe) WHERE active AND dedupe IS NOT NULL
 DO UPDATE SET id=dogrs_queue_jobs_v2.id
 RETURNING tenant,id,queue,kind,dedupe
)
SELECT i.ordinal,w.id FROM input i JOIN written w
 ON w.tenant=i.tenant AND (w.id=i.id OR
  (i.dedupe IS NOT NULL AND w.queue=i.queue AND w.kind=i.kind AND w.dedupe=i.dedupe))
ORDER BY i.ordinal
