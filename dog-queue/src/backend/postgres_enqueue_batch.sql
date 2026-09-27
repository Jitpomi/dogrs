WITH input AS MATERIALIZED (
 SELECT * FROM unnest($1::text[],$2::text[],$3::jsonb[],$4::text[],
                      $5::text[],$6::text[],$7::int4[],$8::timestamptz[],$9::bytea[])
 WITH ORDINALITY AS i(tenant,id,state,queue,kind,dedupe,priority,run_at,payload,ordinal)
), stamped AS MATERIALIZED (SELECT clock_timestamp() AS now), inserted AS (
 INSERT INTO dogrs_queue_jobs_v2
  (tenant,id,state,queue,kind,dedupe,priority,created_at,updated_at,eligible_at,lease_until,status,active,payload)
 SELECT i.tenant,i.id,
  jsonb_set(jsonb_set(CASE WHEN i.run_at='1970-01-01 00:00:00+00'::timestamptz
    THEN jsonb_set(i.state,'{record,message,run_at}',to_jsonb(stamped.now))
    ELSE i.state END,'{record,created_at}',to_jsonb(stamped.now)),
              '{record,updated_at}',to_jsonb(stamped.now)),
  i.queue,i.kind,i.dedupe,i.priority,stamped.now,stamped.now,
  CASE WHEN i.run_at='1970-01-01 00:00:00+00'::timestamptz THEN stamped.now ELSE i.run_at END,
  NULL,'enqueued',true,i.payload
 FROM input i CROSS JOIN stamped
 ORDER BY i.tenant,i.queue,i.kind,i.dedupe,i.id
 ON CONFLICT (tenant,queue,kind,dedupe) WHERE active AND dedupe IS NOT NULL
 DO UPDATE SET id=dogrs_queue_jobs_v2.id
 RETURNING tenant,id,queue,kind,dedupe
)
SELECT i.ordinal,j.id FROM input i JOIN inserted j ON j.tenant=i.tenant AND
 ((i.dedupe IS NULL AND j.id=i.id) OR
  (i.dedupe IS NOT NULL AND j.queue=i.queue AND j.kind=i.kind AND j.dedupe=i.dedupe))
ORDER BY i.ordinal
