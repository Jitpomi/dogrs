#!/usr/bin/env python3
"""Bounded hosted acceptance test. Synthetic data only; no billing/email APIs."""
import concurrent.futures, json, os, pathlib, secrets, signal, subprocess, sys, time, urllib.request, urllib.error, uuid

binary = pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve()
backend = sys.argv[1]
output = pathlib.Path(os.environ.get('DOGRS_REPORT_DIR', './hosted-results')).resolve()
output.mkdir(parents=True, exist_ok=True)
env = os.environ.copy()
tenant = 'dogrs-test-' + backend + '-' + uuid.uuid4().hex[:12]
env.update(DOGRS_BACKEND=backend, DOGRS_TEST_TENANT=tenant, DOGRS_TEST_TOKEN=secrets.token_hex(24), DOGRS_TEST_BIND='127.0.0.1:38171')
base = 'http://127.0.0.1:38171/payments'
children=[]; files=[]; metrics={}; checks=[]

def command(role):
    return subprocess.run([str(binary),role],env=env,capture_output=True,text=True,timeout=45,check=True).stdout

def launch(role, **overrides):
    log = open(output / f'{tenant}-{role}-{len(children)}.log','w')
    files.append(log)
    p = subprocess.Popen([str(binary),role],env={**env,**overrides},stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    children.append(p)
    return p

def stop(p, hard=False):
    if p.poll() is None:
        p.send_signal(signal.SIGKILL if hard else signal.SIGINT)
        try: p.wait(timeout=12)
        except subprocess.TimeoutExpired: p.kill();p.wait(timeout=5)

def request(method='GET', tail='', data=None, auth=True):
    headers={'Content-Type':'application/json'}
    if auth: headers['Authorization']='Bearer '+env['DOGRS_TEST_TOKEN']
    req=urllib.request.Request(base+tail,data=None if data is None else json.dumps(data).encode(),headers=headers,method=method)
    with urllib.request.urlopen(req,timeout=35) as response: result=json.load(response)
    if isinstance(result,dict) and 'data' in result and isinstance(result['data'],dict): return result['data']
    return result

def until(fn, timeout=90):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        result=fn()
        if result: return result
        time.sleep(0.35)
    raise AssertionError('Timed out waiting for system invariant')

def ready():
    def poll():
        if api.poll() is not None: raise AssertionError('API exited; inspect its log')
        try: request(auth=False)
        except urllib.error.HTTPError as e: return e.code==401
        except (urllib.error.URLError,ConnectionError): return False
        raise AssertionError('Unauthenticated API request was accepted')
    until(poll,60)

def submit(invoice,mode='normal'):
    return request('POST',data={'invoice':invoice,'mode':mode})['id']

def status(job): return request(tail='/'+job)
def completed(job):
    value=status(job)
    assert value['status'] not in ('failed','canceled'), value
    return value if value['status']=='completed' else None

def effects(): return json.loads(command('inspect'))
def effect(invoice): return next((r for r in effects() if r['invoice']==invoice and r['worker']),None)

def check(name):
    checks.append(name); print(backend+': '+name,flush=True)

report={'backend':backend,'tenant':tenant,'started_at':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())}
try:
    command('init')
    if backend in ('kafka','kafka-rust','rabbitmq','sqs'):
        assert 'BROKER_NOTIFICATION_VERIFIED' in command('probe')
        check('broker notification round trip over verified TLS')
    api=launch('serve');ready();check('unauthenticated HTTP rejected')
    duplicate=submit('duplicate')
    assert submit('duplicate')==duplicate
    check('active job idempotency through DogRS HTTP')
    env['DOGRS_TEST_JOB']=duplicate
    assert 'TENANT_ISOLATION_VERIFIED' in command('isolation')
    check('different tenant cannot read the same job ID')
    canceled=submit('canceled')
    assert request('DELETE','/'+canceled)['canceled']
    check('queued cancellation')
    crash=submit('crash','crash')
    crashing=launch('worker',DOGRS_CRASH_AFTER_EFFECT='1')
    until(lambda:effect('crash'))
    assert status(crash)['status']=='processing'
    stop(crashing,hard=True)
    stop(api,hard=True)
    api=launch('serve');ready()
    assert status(crash)['status']=='processing'
    check('committed jobs survive API and worker process death')
    worker_a=launch('worker');worker_b=launch('worker')
    recovered=until(lambda:completed(crash),90)
    assert recovered['attempts']>=2
    assert effect('crash')['attempts']>=2
    check('crash after effect before ack recovered; effect deduplicated')
    until(lambda:completed(duplicate))
    retry=submit('retry','retry');until(lambda:completed(retry))
    assert effect('retry')['attempts']==2
    check('retryable failure retries and completes')
    permanent=submit('permanent','permanent')
    until(lambda:status(permanent)['status']=='failed')
    assert not effect('permanent')
    check('permanent failure produces no effect')
    long_job=submit('long','long');until(lambda:completed(long_job),90)
    assert status(long_job)['attempts']==1
    check('heartbeat retains lease beyond initial 8-second lease')
    t0=time.monotonic()
    timings=[]
    def enqueue(i):
        start=time.monotonic();job=submit('load-'+str(i));timings.append((time.monotonic()-start)*1000);return job
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool: jobs=list(pool.map(enqueue,range(40)))
    for job in jobs: until(lambda job=job:completed(job),120)
    elapsed=time.monotonic()-t0
    rows=effects()
    loaded=[r for r in rows if r['invoice'].startswith('load-')]
    assert len(loaded)==40 and all(r['worker'] for r in loaded)
    assert not effect('canceled')
    assert len({r['invoice'] for r in rows if r['worker']})==44
    check('40 concurrent submissions complete with no missing or duplicate effects')
    assert len({r['worker'] for r in loaded})>=2, 'both competing worker processes must execute jobs'
    check('independent worker processes both performed work')
    timings.sort()
    metrics.update(load_jobs=40,load_seconds=round(elapsed,3),jobs_per_second=round(40/elapsed,3),enqueue_p50_ms=round(timings[len(timings)//2],2),enqueue_p95_ms=round(timings[int(len(timings)*.95)-1],2))
    stop(worker_a);stop(worker_b);stop(api)
    api=launch('serve');ready()
    assert status(crash)['status']=='completed' and status(canceled)['status']=='canceled'
    check('terminal outcomes survive complete process restart')
    report['result']='passed'
except subprocess.CalledProcessError as exc:
    report['result']='failed';report['error']=str(exc)
    print(exc.stderr, file=sys.stderr)
    raise
except Exception as exc:
    report['result']='failed';report['error']=str(exc)
    raise
finally:
    for child in children: stop(child,hard=True)
    for f in files:f.close()
    report.update(checks=checks,metrics=metrics)
    (output / f'{tenant}.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report),flush=True)
