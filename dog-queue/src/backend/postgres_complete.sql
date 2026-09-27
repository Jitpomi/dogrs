WITH locked AS MATERIALIZED (
 SELECT id,state,status,lease_until FROM dogrs_queue_jobs_v2
 WHERE tenant=$1 AND id=$2 FOR UPDATE
), stamped AS MATERIALIZED (
 SELECT *,clock_timestamp() AS now FROM locked
), checked AS MATERIALIZED (
 SELECT *,CASE
   WHEN status='canceled' THEN 2
   WHEN status IN ('completed','failed') THEN 3
   WHEN status<>'processing' OR state->>'token' IS DISTINCT FROM $3 THEN 4
   WHEN lease_until IS NULL OR lease_until<=now THEN 5
   ELSE 0 END AS failure
 FROM stamped
), updated AS (
 UPDATE dogrs_queue_jobs_v2 j SET
   state=jsonb_set(jsonb_set(jsonb_set(jsonb_set(j.state,
     '{record,status}',jsonb_build_object('Completed',jsonb_build_object('completed_at',checked.now))),
     '{record,updated_at}',to_jsonb(checked.now)),
     '{record,result}',COALESCE(to_jsonb($4::text),'null'::jsonb)),
     '{token}','null'::jsonb),
   status='completed',active=false,lease_until=NULL,updated_at=checked.now
 FROM checked WHERE j.tenant=$1 AND j.id=checked.id AND checked.failure=0
 RETURNING j.id
)
SELECT COALESCE((SELECT failure FROM checked),1),(SELECT count(*) FROM updated)
