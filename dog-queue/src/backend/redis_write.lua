-- All keys share a tenant hash tag. Metadata, payload and indexes commit together.
local t=redis.call('TIME')
local now=t[1]*1000+math.floor(t[2]/1000)
if ARGV[3]=='enqueue' and ARGV[5]~='' then
  local existing=redis.call('HGET',KEYS[7],ARGV[5])
  if existing then return existing end
end
if (redis.call('HGET',KEYS[1],ARGV[1]) or '')~=ARGV[2] then return '' end
if tonumber(ARGV[6])>0 and tonumber(ARGV[6])<=now then return '' end
local row=cjson.decode(ARGV[4])
redis.call('HSET',KEYS[1],ARGV[1],ARGV[4])
if ARGV[3]=='enqueue' then redis.call('HSET',KEYS[2],ARGV[1],ARGV[7]) end
redis.call('ZREM',KEYS[3],ARGV[1])
redis.call('ZREM',KEYS[4],ARGV[1])
redis.call('ZREM',KEYS[5],ARGV[1])
if ARGV[8]=='ready' then
  if row.redis_eligible<=now then redis.call('ZADD',KEYS[4],row.redis_score,ARGV[1])
  else redis.call('ZADD',KEYS[3],row.redis_eligible,ARGV[1]) end
elseif ARGV[8]=='processing' then
  redis.call('ZADD',KEYS[5],ARGV[9],ARGV[1])
elseif ARGV[8]=='terminal' then
  redis.call('ZADD',KEYS[6],ARGV[10],ARGV[1])
end
if ARGV[5]~='' then
  if ARGV[8]=='terminal' then
    if redis.call('HGET',KEYS[7],ARGV[5])==ARGV[1] then redis.call('HDEL',KEYS[7],ARGV[5]) end
  else redis.call('HSET',KEYS[7],ARGV[5],ARGV[1]) end
end
return ARGV[1]
