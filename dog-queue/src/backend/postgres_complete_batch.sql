WITH input AS MATERIALIZED (
 SELECT * FROM unnest($1::text[],$2::text[],$3::text[],$4::text[])
 WITH ORDINALITY AS i(tenant,id,token,result,ordinal)
), locked AS MATERIALIZED (
 SELECT j.tenant,j.id,j.state,j.status,j.lease_until
 FROM dogrs_queue_jobs_v2 j JOIN input i ON j.tenant=i.tenant AND j.id=i.id
 ORDER BY j.tenant,j.id FOR UPDATE OF j
), stamped AS MATERIALIZED (
 SELECT *,clock_timestamp() AS now FROM locked
), checked AS MATERIALIZED (
 SELECT i.*,s.now,CASE
   WHEN s.id IS NULL THEN 1
   WHEN s.status='canceled' THEN 2
   WHEN s.status IN ('completed','failed') THEN 3
   WHEN s.status<>'processing' OR s.state->>'token' IS DISTINCT FROM i.token THEN 4
   WHEN s.lease_until IS NULL OR s.lease_until<=s.now THEN 5
   ELSE 0 END AS failure
 FROM input i LEFT JOIN stamped s ON s.tenant=i.tenant AND s.id=i.id
), updated AS (
 UPDATE dogrs_queue_jobs_v2 j SET
   state=jsonb_set(jsonb_set(jsonb_set(jsonb_set(j.state,
     '{record,status}',jsonb_build_object('Completed',jsonb_build_object('completed_at',checked.now))),
     '{record,updated_at}',to_jsonb(checked.now)),
     '{record,result}',COALESCE(to_jsonb(checked.result),'null'::jsonb)),
     '{token}','null'::jsonb),
   status='completed',active=false,lease_until=NULL,updated_at=checked.now
 FROM checked WHERE j.tenant=checked.tenant AND j.id=checked.id AND checked.failure=0
 RETURNING j.tenant,j.id
)
SELECT checked.ordinal,checked.failure,updated.id IS NOT NULL AS committed
FROM checked LEFT JOIN updated USING (tenant,id) ORDER BY checked.ordinal
