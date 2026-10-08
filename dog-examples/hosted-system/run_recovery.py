#!/usr/bin/env python3
"""Disposable real-server crash/failover tests. Never touches hosted providers.

Run with DOGRS_SYSTEM_BINARY pointing to a release build with redis,nats features.
Only containers/network created by this invocation are killed or removed.
"""
import argparse,json,os,pathlib,platform,re,resource,secrets,socket,subprocess,time,threading,urllib.request
p=argparse.ArgumentParser();p.add_argument('backend',choices=['postgres','redis','nats']);p.add_argument('--report-dir',required=True);p.add_argument('--capacity',action='store_true');p.add_argument('--restore',action='store_true');p.add_argument('--race',action='store_true');p.add_argument('--outage-seconds',type=int,default=3);p.add_argument('--seconds',type=int,default=30);p.add_argument('--rate',type=int,choices=range(1,11),default=None,help='Jobs/second per tenant; capacity default 9 (900/s)');p.add_argument('--bytes',type=int,default=1024);p.add_argument('--admission-mode',choices=['native-payload','native-layout','dogrs-admission']);p.add_argument('--overload-drain-seconds',type=int,choices=[0,30,60,120],default=0);a=p.parse_args()
if a.rate is None:a.rate=9 if a.capacity else 10
if a.rate!=10 and not a.capacity:p.error('--rate requires capacity mode')
if a.admission_mode and not a.capacity:p.error('--admission-mode requires --capacity')
if a.overload_drain_seconds and (not a.capacity or a.admission_mode):p.error('overload drain requires the full queue capacity mode')
wal_init_zero=os.environ.get('DOGRS_PG_WAL_INIT_ZERO','on')
if wal_init_zero not in ('on','off'):p.error('WAL initialization must be on or off')
payload_storage=os.environ.get('DOGRS_PG_PAYLOAD_STORAGE','extended')
if payload_storage not in ('extended','external'):p.error('payload storage must be extended or external')
if payload_storage=='external' and a.admission_mode:p.error('payload storage tuning requires the full queue adapter')
pg_instances=int(os.environ.get('DOGRS_PG_INSTANCES','1'))
if pg_instances not in (1,4):p.error('PostgreSQL instances must be 1 or 4')
if pg_instances>1 and (a.backend!='postgres' or not a.capacity or a.admission_mode or os.environ.get('DOGRS_PG_FIXTURE_PARTITIONS','0')!='0'):p.error('independent PostgreSQL instances require ordinary full queue capacity')
root=pathlib.Path(a.report_dir).resolve();root.mkdir(parents=True,exist_ok=True)
run='dogrs-fault-'+secrets.token_hex(5);folder=root/run;folder.mkdir()
binary=str(pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve());containers=[];network=None;process=None
allocated_ports=set()

def command(*args):return subprocess.check_output(args,text=True).strip()
def port():
 while True:
  with socket.socket() as s:s.bind(('127.0.0.1',0));number=s.getsockname()[1]
  if number not in allocated_ports:
   allocated_ports.add(number);return number
def launch(name,*args):
 command('docker','create','--name',name,*args);containers.append(name)
 command('docker','start',name)
def wait_port(number):
 deadline=time.monotonic()+60
 while True:
  try:
   with socket.create_connection(('127.0.0.1',number),timeout=1):return
  except OSError:
   if time.monotonic()>deadline:raise TimeoutError('service readiness')
   time.sleep(.2)
try:
 (folder/'environment.json').write_text(json.dumps({
  'host_architecture':platform.machine(),'host_logical_cpus':os.cpu_count(),
  'docker':json.loads(command('docker','info','--format','{"cpus":{{.NCPU}},"memory_bytes":{{.MemTotal}},"architecture":"{{.Architecture}}"}')),
  'postgres_enqueue_batch_size':int(os.environ.get('DOGRS_PG_ENQUEUE_BATCH_SIZE','16')),
  'postgres_commit_delay_us':int(os.environ.get('DOGRS_PG_COMMIT_DELAY','0')),
  'postgres_capacity_memory':a.backend=='postgres' and a.capacity,
  'postgres_wait_sampling':os.environ.get('DOGRS_PG_PROFILE')=='1',
  'queue_stage_timings':os.environ.get('DOGRS_QUEUE_TIMINGS')=='1',
  'cpu_sampling':bool(os.environ.get('DOGRS_PERF')),
  'postgres_fixture_partitions':int(os.environ.get('DOGRS_PG_FIXTURE_PARTITIONS','0')),
  'storage_shards':pg_instances if pg_instances>1 else int(os.environ.get('DOGRS_CAPACITY_SHARDS','1')),
  'postgres_instances':pg_instances,
  'postgres_wal_init_zero':wal_init_zero,
  'postgres_payload_storage':payload_storage,
  'postgres_shared_buffers_mb_per_instance':1024//pg_instances if a.backend=='postgres' and a.capacity else None,
  'postgres_max_wal_mb_per_instance':4096//pg_instances if a.backend=='postgres' and a.capacity else None,
  'payload_pattern':'unique-per-tenant-and-sequence' if a.capacity else 'recovery-fixture',
  'measurement':a.admission_mode or ('queue-capacity' if a.capacity else 'recovery'),
  'comparison_tenant':os.environ.get('DOGRS_CAPACITY_COMPARISON_TENANT') if a.capacity else None,
  'nats_connections':os.environ.get('DOGRS_NATS_CONNECTIONS','shared'),
  'nats_atomic':os.environ.get('DOGRS_NATS_ATOMIC')!='0',
  'nats_image':os.environ.get('DOGRS_NATS_IMAGE','nats:2.15.0-alpine'),
  'nats_storage':'anonymous Docker volume at /data',
 },indent=2))
 env={**os.environ,'DOGRS_BACKEND':a.backend,'DOGRS_TEST_TENANT':run.replace('dogrs-fault-','dogrs-test-fault-'),'DOGRS_RECOVERY_MANIFEST':str(folder/'manifest.json')}
 env.pop('DOGRS_ADMISSION_MODE',None)
 if a.capacity and os.environ.get('DOGRS_CAPACITY_COMPARISON_TENANT'):
  env['DOGRS_TEST_TENANT']=os.environ['DOGRS_CAPACITY_COMPARISON_TENANT']
 monitors={};profile_ports={}
 if a.backend=='postgres':
  pg_nodes=[];urls=[]
  for node in range(pg_instances):
   number=port();name=run+'-pg'+(str(node) if pg_instances>1 else '')
   node_folder=folder/f'pg-{node}' if pg_instances>1 else folder;node_folder.mkdir(exist_ok=True)
   pg_nodes.append((name,node_folder))
   pg_tuning=['-c',f'shared_buffers={1024//pg_instances}MB','-c',f'max_wal_size={4096//pg_instances}MB','-c','checkpoint_timeout=15min'] if a.capacity else []
   delay=int(os.environ.get('DOGRS_PG_COMMIT_DELAY','0'));assert 0<=delay<=10000
   pg_tuning+=['-c',f'wal_init_zero={wal_init_zero}','-c',f'commit_delay={delay}','-c','commit_siblings=1','-c','track_wal_io_timing=on']
   launch(name,'-p',f'127.0.0.1:{number}:5432','-e','POSTGRES_PASSWORD=disposable-only','postgres:18-alpine','postgres','-c','shared_preload_libraries=pg_stat_statements','-c','track_io_timing=on',*pg_tuning)
   urls.append(f'host=127.0.0.1 port={number} user=postgres password=disposable-only dbname=postgres')
   wait_port(number)
   deadline=time.monotonic()+60
   while subprocess.run(['docker','exec',name,'pg_isready','-h','127.0.0.1','-U','postgres'],capture_output=True).returncode:
    assert time.monotonic()<deadline;time.sleep(.2)
   command('docker','exec',name,'psql','-U','postgres','-c','CREATE EXTENSION pg_stat_statements')
  env['DOGRS_POSTGRES_URL']=urls[0]
  if pg_instances>1:env.update(DOGRS_PG_SHARD_URLS=json.dumps(urls),DOGRS_CAPACITY_SHARDS=str(pg_instances))
  partitions=int(os.environ.get('DOGRS_PG_FIXTURE_PARTITIONS','0'))
  assert partitions in (0,16),'unsupported diagnostic partition count'
  if partitions:
   assert a.capacity and not a.admission_mode,'partition experiment requires full queue capacity'
   schema=(pathlib.Path(__file__).resolve().parents[2]/'dog-queue/src/backend/postgres_schema.sql').read_text()
   create=schema.split(';',1)[0]+' PARTITION BY HASH (tenant);'
   create+='\n'.join(f'CREATE TABLE dogrs_queue_jobs_v2_p{i} PARTITION OF dogrs_queue_jobs_v2 FOR VALUES WITH (MODULUS {partitions},REMAINDER {i});' for i in range(partitions))
   command('docker','exec',name,'psql','-v','ON_ERROR_STOP=1','-U','postgres','-c',create)
 elif a.backend=='redis':
  number=port();name=run+'-redis'
  launch(name,'-p',f'127.0.0.1:{number}:6379','redis:7.4-alpine','redis-server','--appendonly','yes','--appendfsync','always','--maxmemory-policy','noeviction','--latency-monitor-threshold','1')
  env['DOGRS_REDIS_URL']=f'redis://127.0.0.1:{number}/';env['DOGRS_REDIS_REQUIRE_AOF']='1';wait_port(number)
 else:
  network=run;command('docker','network','create',network)
  names=[run+'-n'+str(i) for i in range(3)];numbers=[port() for _ in names];monitor_ports=[port() for _ in names]
  for name,number,monitor in zip(names,numbers,monitor_ports):
   config=folder/(name+'.conf');routes=','.join('"nats://'+n+':6222"' for n in names if n!=name)
   config.write_text(f'server_name: {name}\nport: 4222\nhttp: 8222\nclient_advertise: "127.0.0.1:{number}"\njetstream {{store_dir:"/data",sync_interval:always}}\ncluster {{name:"{run}",listen:"0.0.0.0:6222",routes:[{routes}]}}\n')
   profile_args=[];profile_command=[]
   if a.capacity and os.environ.get('DOGRS_QUEUE_TIMINGS')=='1':
    profile_port=port();profile_ports[name]=profile_port
    profile_args=['-p',f'127.0.0.1:{profile_port}:6543'];profile_command=['--profile','6543']
   launch(name,*profile_args,'--network',network,'-v','/data','-p',f'127.0.0.1:{number}:4222','-p',f'127.0.0.1:{monitor}:8222','-v',f'{config}:/etc/nats.conf:ro',os.environ.get('DOGRS_NATS_IMAGE','nats:2.15.0-alpine'),'-c','/etc/nats.conf',*profile_command)
   monitors[name]=monitor
  for number in numbers:wait_port(number)
  deadline=time.monotonic()+60
  while True:
   try:
    with urllib.request.urlopen(f'http://127.0.0.1:{monitor_ports[0]}/jsz',timeout=2) as response:ready=json.load(response)
    if ready.get('meta_cluster',{}).get('leader') and ready['meta_cluster'].get('cluster_size')==3:
     # A leader can exist before all peer advertisements reach its placement view.
     # Require every node to report the same elected leader before creating R3 assets.
     states=[]
     for monitor in monitor_ports:
      with urllib.request.urlopen(f'http://127.0.0.1:{monitor}/jsz',timeout=2) as response:states.append(json.load(response))
     if all(s.get('meta_cluster',{}).get('leader')==ready['meta_cluster']['leader'] and s.get('meta_cluster',{}).get('cluster_size')==3 for s in states):
      time.sleep(2);break
   except (OSError,ValueError):pass
   if time.monotonic()>deadline:raise TimeoutError('JetStream cluster readiness')
   time.sleep(.2)
  env.update(DOGRS_NATS_URL=','.join('nats://127.0.0.1:'+str(n) for n in numbers),DOGRS_NATS_BUCKET=run.replace('-','_'),DOGRS_NATS_REPLICAS='3')
 if a.race:
  with (folder/'race.log').open('w') as output:
   result=subprocess.run([binary,'recovery-race'],env=env,stdout=output,stderr=subprocess.STDOUT,timeout=180)
  print(json.dumps({'backend':a.backend,'race_passed':result.returncode==0,'log':str(folder/'race.log')}))
  raise SystemExit(result.returncode)
 if a.capacity:
  if a.admission_mode:env['DOGRS_ADMISSION_MODE']=a.admission_mode
  env.update(DOGRS_CAPACITY_RATE=str(a.rate),DOGRS_CAPACITY_RECOVERY_SECONDS=str(a.overload_drain_seconds),DOGRS_CAPACITY_TENANTS='100',DOGRS_CAPACITY_SECONDS=str(a.seconds),DOGRS_CAPACITY_BYTES=str(a.bytes))
  before=resource.getrusage(resource.RUSAGE_CHILDREN)
  with (folder/'container-stats.jsonl').open('w') as stats:
   monitor=subprocess.Popen(['docker','stats','--format','{{json .}}',*containers],stdout=stats,stderr=subprocess.DEVNULL)
   samplers=[];profile_stop=threading.Event();profile_thread=None
   def sample_nats_stacks():
    sample=0
    while not profile_stop.is_set():
     for name,number in profile_ports.items():
      try:
       with urllib.request.urlopen(f'http://127.0.0.1:{number}/debug/pprof/goroutine?debug=2',timeout=2) as response:data=response.read()
       (folder/f'{name}-goroutines-{sample:03d}.txt').write_bytes(data)
      except OSError as error:
       (folder/f'{name}-goroutines-{sample:03d}.error').write_text(str(error))
     sample+=1
     profile_stop.wait(5)
   try:
    if profile_ports:
     profile_thread=threading.Thread(target=sample_nats_stacks,daemon=True);profile_thread.start()
    if a.backend=='postgres' and os.environ.get('DOGRS_PG_PROFILE')=='1':
     for node_name,node_folder in pg_nodes:
      sample_output=(node_folder/'postgres-waits.log').open('w')
      sampler=subprocess.Popen(['docker','exec','-i',node_name,'psql','-XAt','-U','postgres'],stdin=subprocess.PIPE,stdout=sample_output,stderr=subprocess.STDOUT,text=True)
      samplers.append((sampler,sample_output))
      sampler.stdin.write("SELECT jsonb_build_object('at',clock_timestamp(),'waits',COALESCE(jsonb_agg(s),'[]'::jsonb)) FROM (SELECT CASE WHEN query LIKE '%INSERT INTO dogrs_queue_jobs_v2%' THEN 'enqueue' WHEN query LIKE '%WITH input%' AND query LIKE '%jsonb[]%' THEN 'claim' WHEN query LIKE '%WITH input%' THEN 'complete' ELSE left(query,80) END AS operation,wait_event_type,wait_event,CASE WHEN wait_event='extend' THEN (SELECT relation::regclass::text FROM pg_locks l WHERE l.pid=a.pid AND l.locktype='extend' AND NOT l.granted LIMIT 1) END AS relation,count(*) AS backends FROM pg_stat_activity a WHERE pid<>pg_backend_pid() AND datname=current_database() AND state='active' GROUP BY 1,2,3,4) s;\n\\watch 0.1\n")
      sampler.stdin.close()
    with (folder/'capacity.log').open('w') as output:
     role='admission-native' if a.admission_mode and a.admission_mode.startswith('native-') else 'capacity-local'
     invocation=[binary,role]
     profiler=os.environ.get('DOGRS_PERF')
     if profiler:
      assert platform.system()=='Linux','CPU sampling requires the disposable Linux runner'
      invocation=['sudo','-n','-E',profiler,'record','-a','-e','cpu-clock','-F','49','--buildid-all','-o',str(folder/'perf.data'),'--',*invocation]
     result=subprocess.run(invocation,env=env,stdout=output,stderr=subprocess.STDOUT,timeout=300)
     if profiler:
      command('sudo','-n','chmod','0644',str(folder/'perf.data'))
      (folder/'cpu-profile.txt').write_text(command('sudo','-n',profiler,'report','--stdio','--no-children','--sort','comm,dso,symbol','--percent-limit','0.5','-i',str(folder/'perf.data')))
    after=resource.getrusage(resource.RUSAGE_CHILDREN)
    (folder/'client-cpu.json').write_text(json.dumps({'user_seconds':after.ru_utime-before.ru_utime,'system_seconds':after.ru_stime-before.ru_stime}))
   finally:
    profile_stop.set()
    if profile_thread:profile_thread.join(timeout=10)
    for sampler,sample_output in samplers:
     sampler.terminate()
     try:sampler.wait(timeout=5)
     except subprocess.TimeoutExpired:sampler.kill();sampler.wait()
     sample_output.close()
    monitor.terminate()
    try:monitor.wait(timeout=5)
    except subprocess.TimeoutExpired:monitor.kill();monitor.wait()
  stats_path=folder/'container-stats.jsonl'
  clean=re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', stats_path.read_text())
  samples=[json.loads(line) for line in clean.splitlines() if line.strip()]
  stats_path.write_text(''.join(json.dumps(sample)+'\n' for sample in samples))
  if a.backend=='nats':
   # After timed work and verification, record each replica independently.
   # Stream sequences count logical records, not Raft entries or fsyncs.
   for node_name,monitor_port in monitors.items():
    try:
     with urllib.request.urlopen(f'http://127.0.0.1:{monitor_port}/jsz?accounts=true&streams=true&config=true',timeout=5) as response:state=json.load(response)
     (folder/f'{node_name}-stream-state.json').write_text(json.dumps(state))
    except (OSError,ValueError) as error:
     (folder/f'{node_name}-stream-state-error.txt').write_text(str(error))
  if a.backend=='postgres':
   for node_name,node_folder in pg_nodes:
    (node_folder/'durability-settings.txt').write_text(command('docker','exec',node_name,'psql','-U','postgres','-c',"SELECT name,setting FROM pg_settings WHERE name IN ('fsync','synchronous_commit','full_page_writes','wal_init_zero','wal_recycle','wal_sync_method') ORDER BY name"))
    (node_folder/'payload-storage.txt').write_text(command('docker','exec',node_name,'psql','-U','postgres','-c',"SELECT n.nspname,c.relname,a.attstorage,a.attcompression FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relname='dogrs_queue_jobs_v2' AND a.attname='payload' AND NOT a.attisdropped"))
    (node_folder/'io-profile.txt').write_text(command('docker','exec',node_name,'psql','-U','postgres','-c',"SELECT * FROM pg_stat_io WHERE object='wal'; SELECT * FROM pg_stat_wal;"))
    (node_folder/'query-profile.txt').write_text(command('docker','exec',node_name,'psql','-U','postgres','-c',"SELECT left(query,180) AS query,calls,round(mean_exec_time::numeric,3) AS mean_ms,round(total_exec_time::numeric,1) AS total_ms,shared_blks_read,shared_blks_hit,wal_bytes FROM pg_stat_statements ORDER BY total_exec_time DESC LIMIT 12"))
  elif a.backend=='redis':
   # Collect provider costs without changing its persistence or rewrite policy.
   for section in ('persistence','stats','memory','commandstats','latencystats'):
    (folder/f'redis-{section}.txt').write_text(command('docker','exec',name,'redis-cli','INFO',section))
   # Server-side events separate persistence stalls from command execution.
   # Do not capture SLOWLOG arguments: queue payloads can contain private data.
   events=json.loads(command('docker','exec',name,'redis-cli','--json','LATENCY','LATEST'))
   (folder/'redis-latency-events.json').write_text(json.dumps(events,indent=2))
   histories={event[0]:json.loads(command('docker','exec',name,'redis-cli','--json','LATENCY','HISTORY',event[0])) for event in events}
   (folder/'redis-latency-history.json').write_text(json.dumps(histories,indent=2))
   (folder/'redis-durability-settings.txt').write_text(command('docker','exec',name,'redis-cli','CONFIG','GET','appendfsync','appendonly','auto-aof-rewrite-percentage','auto-aof-rewrite-min-size','no-appendfsync-on-rewrite','maxmemory-policy'))
  print(json.dumps({'backend':a.backend,'restored_to_fresh_container':a.restore,'outage_seconds':a.outage_seconds,'replicas':3 if a.backend=='nats' else 1,'sync_policy':'always' if a.backend in ('redis','nats') else 'PostgreSQL default fsync/synchronous_commit','measurement':a.admission_mode or 'queue-capacity','passed':result.returncode==0,'capacity_log':str(folder/'capacity.log')}))
  raise SystemExit(result.returncode)
 log=folder/'client.log'
 with log.open('w') as output:
  process=subprocess.Popen([binary,'recovery-live'],env=env,stdout=output,stderr=subprocess.STDOUT)
  deadline=time.monotonic()+90
  while 'RECOVERY_READY' not in log.read_text():
   if process.poll() is not None:raise RuntimeError('seed failed; inspect client.log')
   if time.monotonic()>deadline:raise TimeoutError('seed deadline')
   time.sleep(.1)
  if a.backend=='nats':
   # Monitoring identifies the actual stream leader rather than choosing a node.
   with urllib.request.urlopen(f'http://127.0.0.1:{next(iter(monitors.values()))}/jsz?accounts=true&streams=true&config=true') as r:state=json.load(r)
   (folder/'before-failover.json').write_text(json.dumps(state,indent=2))
   leaders=[]
   for account in state.get('account_details',[]):
    for stream in account.get('stream_detail',[]):
     if stream.get('name')=='KV_'+env['DOGRS_NATS_BUCKET']:leaders.append(stream['cluster']['leader'])
   assert len(leaders)==1 and leaders[0] in containers,'could not identify stream leader'
   name=leaders[0]
  command('docker','kill','--signal','KILL',name)
  assert 3<=a.outage_seconds<=300
  time.sleep(a.outage_seconds)
  if a.restore:
   assert a.backend in ('postgres','redis'),'restore fixture supports PostgreSQL and Redis'
   source='/data' if a.backend=='redis' else '/var/lib/postgresql/18/docker'
   backup=folder/'cold-backup';backup.mkdir()
   command('docker','cp',name+':'+source+'/.',str(backup))
   with (folder/(name+'-original.log')).open('w') as output:subprocess.run(['docker','logs',name],stdout=output,stderr=subprocess.STDOUT)
   command('docker','rm','-v',name);containers.remove(name)
   if a.backend=='redis':
    launch(name,'-p',f'127.0.0.1:{number}:6379','-v',f'{backup}:/data','redis:7.4-alpine','redis-server','--appendonly','yes','--appendfsync','always','--maxmemory-policy','noeviction')
   else:
    launch(name,'-p',f'127.0.0.1:{number}:5432','-e','POSTGRES_PASSWORD=disposable-only','-e','PGDATA=/var/lib/postgresql/data','-v',f'{backup}:/var/lib/postgresql/data','postgres:18-alpine')
  elif a.backend!='nats':command('docker','start',name)
  # NATS must elect another leader and recover with the former leader still down.
  pathlib.Path(env['DOGRS_RECOVERY_MANIFEST']).with_suffix('.resume').write_text('resume\n')
  result=process.wait(timeout=90)
  assert result==0,'recovery failed; inspect client.log'
  report={'backend':a.backend,'failure':'kill active stream leader; keep it down' if a.backend=='nats' else 'SIGKILL then restart same data directory','restored_to_fresh_container':a.restore,'outage_seconds':a.outage_seconds,'replicas':3 if a.backend=='nats' else 1,'sync_policy':'always' if a.backend in ('nats','redis') else 'PostgreSQL default fsync/synchronous_commit','passed':True,'client_log':str(log)}
  (folder/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
finally:
 if process is not None and process.poll() is None:process.kill();process.wait()
 for name in containers:
  with (folder/(name+'.log')).open('w') as out:subprocess.run(['docker','logs',name],stdout=out,stderr=subprocess.STDOUT)
  subprocess.run(['docker','rm','-f','-v',name],capture_output=True)
 if network:subprocess.run(['docker','network','rm',network],capture_output=True)
