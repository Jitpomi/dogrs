-- Atomic completion: no client-side read/CAS round trip, no payload transfer.
-- Every accessed key is explicitly supplied and shares the tenant hash tag.
local raw=redis.call('HGET',KEYS[1],ARGV[1])
if not raw then return 1 end
local row=cjson.decode(raw)
local record=row.record
local status=record.status
if type(status)=='table' and status.Canceled then return 2 end
if type(status)=='table' and (status.Completed or status.Failed) then return 3 end
if type(status)~='table' or not status.Processing or row.token~=ARGV[2] then return 4 end
local t=redis.call('TIME')
local now=t[1]*1000+math.floor(t[2]/1000)
-- The lease index is updated atomically with Processing metadata by every writer.
local until_ms=tonumber(redis.call('ZSCORE',KEYS[2],ARGV[1]))
if not until_ms or until_ms<=now then return 5 end
if #ARGV[3]>4096 then return 6 end

-- Convert the authoritative Redis clock to RFC3339 UTC. Lua's Redis sandbox
-- has no os.date. Gregorian civil-from-days handles leap years and centuries.
local days=math.floor(tonumber(t[1])/86400)
local z=days+719468
local era=math.floor(z/146097)
local doe=z-era*146097
local yoe=math.floor((doe-math.floor(doe/1460)+math.floor(doe/36524)-math.floor(doe/146096))/365)
local year=yoe+era*400
local doy=doe-(365*yoe+math.floor(yoe/4)-math.floor(yoe/100))
local mp=math.floor((5*doy+2)/153)
local day=doy-math.floor((153*mp+2)/5)+1
local month=mp+(mp<10 and 3 or -9)
if month<=2 then year=year+1 end
local secs=tonumber(t[1])-days*86400
local stamp=string.format('%04d-%02d-%02dT%02d:%02d:%02d.%03dZ',
  year,month,day,math.floor(secs/3600),math.floor(secs/60)%60,secs%60,math.floor(t[2]/1000))
record.status={Completed={completed_at=stamp}}
record.updated_at=stamp
record.result=cjson.decode(ARGV[3])
row.token=cjson.null

-- Redis cjson turns an empty Lua table into {}, losing JSON's [] distinction.
-- Serialize the only array field explicitly, including legacy nonempty bytes.
-- Compose object members, never substitute substrings in user-controlled text.
local message=record.message
local bytes={}
for i,b in ipairs(message.payload_bytes) do bytes[i]=tostring(b) end
message.payload_bytes=nil
local message_json=cjson.encode(message)
message_json=string.sub(message_json,1,-2)..',"payload_bytes":['..table.concat(bytes,',')..']}'
record.message=nil
local record_json=cjson.encode(record)
record_json=string.sub(record_json,1,-2)..',"message":'..message_json..'}'
row.record=nil
local row_json=cjson.encode(row)
row_json=string.sub(row_json,1,-2)..',"record":'..record_json..'}'
local dedupe_key=nil
if message.idempotency_key~=cjson.null and message.idempotency_key~=nil then
  -- Rust serde_json emits UTF-8 and does not escape '/'; cjson normally does.
  local key=cjson.encode({message.queue,message.job_type,message.idempotency_key})
  key=string.gsub(key,'\\/','/')
  dedupe_key=key
end

-- All decoding/encoding and validation finishes before the first mutation.
redis.call('HSET',KEYS[1],ARGV[1],row_json)
redis.call('ZREM',KEYS[2],ARGV[1])
redis.call('ZADD',KEYS[3],now,ARGV[1])
if dedupe_key and redis.call('HGET',KEYS[4],dedupe_key)==ARGV[1] then
  redis.call('HDEL',KEYS[4],dedupe_key)
end
return 0
