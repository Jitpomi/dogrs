#!/usr/bin/env python3
"""Disposable real-server crash/failover tests. Never touches hosted providers.

Run with DOGRS_SYSTEM_BINARY pointing to a release build with redis,nats features.
Only containers/network created by this invocation are killed or removed.
"""
import argparse,json,os,pathlib,secrets,socket,subprocess,time,urllib.request
p=argparse.ArgumentParser();p.add_argument('backend',choices=['postgres','redis','nats']);p.add_argument('--report-dir',required=True);p.add_argument('--capacity',action='store_true');p.add_argument('--seconds',type=int,default=30);p.add_argument('--bytes',type=int,default=1024);a=p.parse_args()
root=pathlib.Path(a.report_dir).resolve();root.mkdir(parents=True,exist_ok=True)
run='dogrs-fault-'+secrets.token_hex(5);folder=root/run;folder.mkdir()
binary=str(pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve());containers=[];network=None;process=None

def command(*args):return subprocess.check_output(args,text=True).strip()
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
def launch(name,*args):
 command('docker','run','-d','--name',name,*args);containers.append(name)
def wait_port(number):
 deadline=time.monotonic()+60
 while True:
  try:
   with socket.create_connection(('127.0.0.1',number),timeout=1):return
  except OSError:
   if time.monotonic()>deadline:raise TimeoutError('service readiness')
   time.sleep(.2)
try:
 env={**os.environ,'DOGRS_BACKEND':a.backend,'DOGRS_TEST_TENANT':run.replace('dogrs-fault-','dogrs-test-fault-'),'DOGRS_RECOVERY_MANIFEST':str(folder/'manifest.json')}
 monitors={}
 if a.backend=='postgres':
  number=port();name=run+'-pg'
  launch(name,'-p',f'127.0.0.1:{number}:5432','-e','POSTGRES_PASSWORD=disposable-only','postgres:18-alpine')
  env['DOGRS_POSTGRES_URL']=f'host=127.0.0.1 port={number} user=postgres password=disposable-only dbname=postgres'
  wait_port(number)
  deadline=time.monotonic()+60
  while subprocess.run(['docker','exec',name,'pg_isready','-h','127.0.0.1','-U','postgres'],capture_output=True).returncode:
   assert time.monotonic()<deadline;time.sleep(.2)
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
   launch(name,'--network',network,'-p',f'127.0.0.1:{number}:4222','-p',f'127.0.0.1:{monitor}:8222','-v',f'{config}:/etc/nats.conf:ro','nats:2.11-alpine','-c','/etc/nats.conf')
   monitors[name]=monitor
  for number in numbers:wait_port(number)
  deadline=time.monotonic()+60
  while True:
   try:
    with urllib.request.urlopen(f'http://127.0.0.1:{monitor_ports[0]}/jsz',timeout=2) as response:ready=json.load(response)
    if ready.get('meta_cluster',{}).get('leader') and ready['meta_cluster'].get('cluster_size')==3:break
   except (OSError,ValueError):pass
   if time.monotonic()>deadline:raise TimeoutError('JetStream cluster readiness')
   time.sleep(.2)
  env.update(DOGRS_NATS_URL=','.join('nats://127.0.0.1:'+str(n) for n in numbers),DOGRS_NATS_BUCKET=run.replace('-','_'),DOGRS_NATS_REPLICAS='3')
 if a.capacity:
  env.update(DOGRS_CAPACITY_TENANTS='100',DOGRS_CAPACITY_SECONDS=str(a.seconds),DOGRS_CAPACITY_BYTES=str(a.bytes))
  with (folder/'capacity.log').open('w') as output:
   result=subprocess.run([binary,'capacity-local'],env=env,stdout=output,stderr=subprocess.STDOUT,timeout=300)
  print(json.dumps({'backend':a.backend,'replicas':3 if a.backend=='nats' else 1,'sync_policy':'always' if a.backend in ('redis','nats') else 'PostgreSQL default fsync/synchronous_commit','passed':result.returncode==0,'capacity_log':str(folder/'capacity.log')}))
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
  time.sleep(3)
  if a.backend!='nats':command('docker','start',name)
  # NATS must elect another leader and recover with the former leader still down.
  pathlib.Path(env['DOGRS_RECOVERY_MANIFEST']).with_suffix('.resume').write_text('resume\n')
  result=process.wait(timeout=90)
  assert result==0,'recovery failed; inspect client.log'
  report={'backend':a.backend,'failure':'kill active stream leader; keep it down' if a.backend=='nats' else 'SIGKILL then restart same data directory','replicas':3 if a.backend=='nats' else 1,'sync_policy':'always' if a.backend in ('nats','redis') else 'PostgreSQL default fsync/synchronous_commit','passed':True,'client_log':str(log)}
  (folder/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
finally:
 if process is not None and process.poll() is None:process.kill();process.wait()
 for name in containers:
  with (folder/(name+'.log')).open('w') as out:subprocess.run(['docker','logs',name],stdout=out,stderr=subprocess.STDOUT)
  subprocess.run(['docker','rm','-f','-v',name],capture_output=True)
 if network:subprocess.run(['docker','network','rm',network],capture_output=True)
