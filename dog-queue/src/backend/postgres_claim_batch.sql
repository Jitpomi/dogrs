WITH input AS MATERIALIZED (
 SELECT * FROM unnest($1::text[],$2::jsonb[],$3::text[],$4::float8[])
 WITH ORDINALITY AS i(tenant,queues,token,seconds,ordinal)
), picked AS MATERIALIZED (
 SELECT i.*,candidate.id FROM input i
 LEFT JOIN LATERAL (
  SELECT j.id FROM dogrs_queue_jobs_v2 j
  WHERE j.tenant=i.tenant AND j.queue=ANY(ARRAY(SELECT jsonb_array_elements_text(i.queues)))
    AND j.eligible_at<=statement_timestamp() AND j.status IN ('enqueued','retrying')
  ORDER BY j.priority DESC,j.created_at,j.id LIMIT 1 FOR UPDATE SKIP LOCKED
 ) candidate ON true
), stamped AS MATERIALIZED (
 SELECT *,clock_timestamp() AS now FROM picked WHERE id IS NOT NULL
), leased AS (
 UPDATE dogrs_queue_jobs_v2 j SET
  state=jsonb_set(jsonb_set(jsonb_set(jsonb_set(j.state,
   '{record,status}',jsonb_build_object('Processing',jsonb_build_object('lease_until',stamped.now+interval '1 second'*stamped.seconds))),
   '{record,attempt}',to_jsonb((j.state#>>'{record,attempt}')::bigint+1)),
   '{record,updated_at}',to_jsonb(stamped.now)),
   '{token}',to_jsonb(stamped.token)),
  status='processing',updated_at=stamped.now,lease_until=stamped.now+interval '1 second'*stamped.seconds
 FROM stamped WHERE j.tenant=stamped.tenant AND j.id=stamped.id
 RETURNING j.tenant,j.id,j.state,j.payload
)
SELECT picked.ordinal,leased.state,leased.payload
FROM picked LEFT JOIN leased USING (tenant,id) ORDER BY picked.ordinal
