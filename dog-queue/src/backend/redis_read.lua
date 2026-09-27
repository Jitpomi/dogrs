local t=redis.call('TIME')
local now=t[1]*1000+math.floor(t[2]/1000)
local result={tostring(now)}
if ARGV[1]=='claim' then
  for n=3,#KEYS,2 do
    local due=redis.call('ZRANGEBYSCORE',KEYS[n],'-inf',now,'LIMIT',0,256)
    for _,id in ipairs(due) do
      local raw=redis.call('HGET',KEYS[1],id)
      if raw then
        local row=cjson.decode(raw)
        redis.call('ZADD',KEYS[n+1],row.redis_score,id)
      end
      redis.call('ZREM',KEYS[n],id)
    end
    local ids=redis.call('ZRANGE',KEYS[n+1],0,0)
    if #ids>0 then
      local raw=redis.call('HGET',KEYS[1],ids[1])
      if raw then table.insert(result,raw) end
    end
  end
elseif ARGV[1]=='reap' then
  for _,id in ipairs(redis.call('ZRANGEBYSCORE',KEYS[2],'-inf',now,'LIMIT',0,256)) do
    local raw=redis.call('HGET',KEYS[1],id)
    if raw then table.insert(result,raw) end
  end
else
  for n=2,#ARGV do
    table.insert(result,redis.call('HGET',KEYS[1],ARGV[n]) or '')
  end
end
return result
