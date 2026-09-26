#!/usr/bin/env python3
"""Disposable real-server crash/failover tests. Never touches hosted providers.

Run with DOGRS_SYSTEM_BINARY pointing to a release build with redis,nats features.
Only containers/network created by this invocation are killed or removed.
"""
import argparse,json,os,pathlib,platform,re,resource,secrets,socket,subprocess,time,urllib.request
p=argparse.ArgumentParser();p.add_argument('backend',choices=['postgres','redis','nats']);p.add_argument('--report-dir',required=True);p.add_argument('--capacity',action='store_true');p.add_argument('--restore',action='store_true');p.add_argument('--race',action='store_true');p.add_argument('--outage-seconds',type=int,default=3);p.add_argument('--seconds',type=int,default=30);p.add_argument('--bytes',type=int,default=1024);a=p.parse_args()
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
  'postgres_commit_delay_us':int(os.environ.get('DOGRS_PG_COMMIT_DELAY','0')),
  'postgres_capacity_memory':a.backend=='postgres' and a.capacity,
  'nats_image':os.environ.get('DOGRS_NATS_IMAGE','nats:2.11-alpine'),
 },indent=2))
 env={**os.environ,'DOGRS_BACKEND':a.backend,'DOGRS_TEST_TENANT':run.replace('dogrs-fault-','dogrs-test-fault-'),'DOGRS_RECOVERY_MANIFEST':str(folder/'manifest.json')}
 monitors={}
 if a.backend=='postgres':
  number=port();name=run+'-pg'
  pg_tuning=['-c','shared_buffers=1GB','-c','max_wal_size=4GB','-c','checkpoint_timeout=15min'] if a.capacity else []
  delay=int(os.environ.get('DOGRS_PG_COMMIT_DELAY','0'));assert 0<=delay<=10000
  pg_tuning+=['-c',f'commit_delay={delay}','-c','commit_siblings=1','-c','track_wal_io_timing=on']
  launch(name,'-p',f'127.0.0.1:{number}:5432','-e','POSTGRES_PASSWORD=disposable-only','postgres:18-alpine','postgres','-c','shared_preload_libraries=pg_stat_statements','-c','track_io_timing=on',*pg_tuning)
  env['DOGRS_POSTGRES_URL']=f'host=127.0.0.1 port={number} user=postgres password=disposable-only dbname=postgres'
  wait_port(number)
  deadline=time.monotonic()+60
  while subprocess.run(['docker','exec',name,'pg_isready','-h','127.0.0.1','-U','postgres'],capture_output=True).returncode:
   assert time.monotonic()<deadline;time.sleep(.2)
  command('docker','exec',name,'psql','-U','postgres','-c','CREATE EXTENSION pg_stat_statements')
 elif a.backend=='redis':
  number=port();name=run+'-redis'
  launch(name,'-p',f'127.0.0.1:{number}:6379','redis:7.4-alpine','redis-server','--appendonly','yes','--appendfsync','always','--maxmemory-policy','noeviction')
  env['DOGRS_REDIS_URL']=f'redis://127.0.0.1:{number}/';env['DOGRS_REDIS_REQUIRE_AOF']='1';wait_port(number)
 else:
  network=run;command('docker','network','create',network)
  names=[run+'-n'+str(i) for i in range(3)];numbers=[port() for _ in names];monitor_ports=[port() for _ in names]
  for name,number,monitor in zip(names,numbers,monitor_ports):
   config=folder/(name+'.conf');routes=','.join('"nats://'+n+':6222"' for n in names if n!=name)
   config.write_text(f'server_name: {name}\nport: 4222\nhttp: 8222\nclient_advertise: "127.0.0.1:{number}"\njetstream {{store_dir:"/data",sync_interval:always}}\ncluster {{name:"{run}",listen:"0.0.0.0:6222",routes:[{routes}]}}\n')
   launch(name,'--network',network,'-p',f'127.0.0.1:{number}:4222','-p',f'127.0.0.1:{monitor}:8222','-v',f'{config}:/etc/nats.conf:ro',os.environ.get('DOGRS_NATS_IMAGE','nats:2.11-alpine'),'-c','/etc/nats.conf')
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
  env.update(DOGRS_CAPACITY_TENANTS='100',DOGRS_CAPACITY_SECONDS=str(a.seconds),DOGRS_CAPACITY_BYTES=str(a.bytes))
  before=resource.getrusage(resource.RUSAGE_CHILDREN)
  with (folder/'container-stats.jsonl').open('w') as stats:
   monitor=subprocess.Popen(['docker','stats','--format','{{json .}}',*containers],stdout=stats,stderr=subprocess.DEVNULL)
   try:
    with (folder/'capacity.log').open('w') as output:
     result=subprocess.run([binary,'capacity-local'],env=env,stdout=output,stderr=subprocess.STDOUT,timeout=300)
    after=resource.getrusage(resource.RUSAGE_CHILDREN)
    (folder/'client-cpu.json').write_text(json.dumps({'user_seconds':after.ru_utime-before.ru_utime,'system_seconds':after.ru_stime-before.ru_stime}))
   finally:
    monitor.terminate()
    try:monitor.wait(timeout=5)
    except subprocess.TimeoutExpired:monitor.kill();monitor.wait()
  stats_path=folder/'container-stats.jsonl'
  clean=re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', stats_path.read_text())
  samples=[json.loads(line) for line in clean.splitlines() if line.strip()]
  stats_path.write_text(''.join(json.dumps(sample)+'\n' for sample in samples))
  if a.backend=='postgres':
   (folder/'io-profile.txt').write_text(command('docker','exec',name,'psql','-U','postgres','-c',"SELECT * FROM pg_stat_io WHERE object='wal'; SELECT * FROM pg_stat_wal;"))
   (folder/'query-profile.txt').write_text(command('docker','exec',name,'psql','-U','postgres','-c',"SELECT left(query,180) AS query,calls,round(mean_exec_time::numeric,3) AS mean_ms,round(total_exec_time::numeric,1) AS total_ms,shared_blks_read,shared_blks_hit,wal_bytes FROM pg_stat_statements ORDER BY total_exec_time DESC LIMIT 12"))
  print(json.dumps({'backend':a.backend,'restored_to_fresh_container':a.restore,'outage_seconds':a.outage_seconds,'replicas':3 if a.backend=='nats' else 1,'sync_policy':'always' if a.backend in ('redis','nats') else 'PostgreSQL default fsync/synchronous_commit','passed':result.returncode==0,'capacity_log':str(folder/'capacity.log')}))
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
