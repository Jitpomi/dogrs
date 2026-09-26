WITH picked AS MATERIALIZED (
 SELECT id FROM dogrs_queue_jobs_v2
 WHERE tenant=$1 AND queue=ANY($2)
   AND eligible_at <= statement_timestamp() AND status IN ('enqueued','retrying')
 ORDER BY priority DESC,created_at,id LIMIT 1 FOR UPDATE SKIP LOCKED
), stamped AS MATERIALIZED (
 SELECT id,clock_timestamp() AS now FROM picked
), leased AS (
 SELECT id,now,now+interval '1 second'*$4::double precision AS until FROM stamped
)
UPDATE dogrs_queue_jobs_v2 j SET
 state=jsonb_set(jsonb_set(jsonb_set(jsonb_set(j.state,
   '{record,status}',jsonb_build_object('Processing',jsonb_build_object('lease_until',leased.until))),
   '{record,attempt}',to_jsonb((j.state#>>'{record,attempt}')::bigint+1)),
   '{record,updated_at}',to_jsonb(leased.now)),
   '{token}',to_jsonb($3::text)),
 status='processing',updated_at=leased.now,lease_until=leased.until
FROM leased WHERE j.tenant=$1 AND j.id=leased.id
RETURNING j.state,j.payload
