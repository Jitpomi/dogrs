#!/usr/bin/env python3
"""Bounded, open-loop capacity probe; failed attempts and backlog remain evidence.

Each stage retains prior history, so later stages exercise larger ledgers. This
single-tenant gate must pass before testing the 100-tenant aggregate target.
"""
import argparse
import concurrent.futures
import json
import math
import os
import pathlib
import secrets
import signal
import subprocess
import time
import urllib.request
import urllib.error
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('backend')
parser.add_argument('--rate', type=float, default=10)
parser.add_argument('--seconds', type=int, default=10)
parser.add_argument('--drain-seconds', type=int, default=120)
parser.add_argument('--payloads', default='1024,16384,65536')
args = parser.parse_args()
assert 0 < args.rate <= 100 and 1 <= args.seconds <= 300
payloads = [int(n) for n in args.payloads.split(',')]
assert all(128 <= n <= 65536 for n in payloads)
binary = str(pathlib.Path(os.environ['DOGRS_SYSTEM_BINARY']).resolve())
output = pathlib.Path(os.environ['DOGRS_REPORT_DIR']).resolve()
output.mkdir(parents=True, exist_ok=True)
tenant = f'dogrs-test-capacity-{uuid.uuid4().hex[:12]}'
env = {**os.environ, 'DOGRS_BACKEND': args.backend, 'DOGRS_TEST_TENANT': tenant,
       'DOGRS_TEST_TOKEN': secrets.token_hex(24), 'DOGRS_TEST_BIND': '127.0.0.1:38171',
       'DOGRS_TEST_MAX_PAYLOAD': '65536'}
children, logs = [], []
base = 'http://127.0.0.1:38171/payments'
report = {'tenant': tenant, 'backend': args.backend, 'target_jobs_per_second': args.rate,
          'target_tenants': 100, 'scope': 'single-tenant capacity gate', 'stages': []}


def request(method='GET', tail='', body=None):
    req = urllib.request.Request(base + tail, method=method,
        data=None if body is None else json.dumps(body).encode(),
        headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + env['DOGRS_TEST_TOKEN']})
    with urllib.request.urlopen(req, timeout=30) as response:
        value = json.load(response)
    return value.get('data', value)


def launch(role):
    log = open(output / f'{tenant}-{role}-{len(children)}.log', 'w')
    logs.append(log)
    child = subprocess.Popen([binary, role], env=env, stdout=log, stderr=subprocess.STDOUT)
    children.append(child)
    return child


def inspect():
    return json.loads(subprocess.run([binary, 'inspect'], env=env, capture_output=True,
        text=True, check=True, timeout=45).stdout)


try:
    subprocess.run([binary, 'init'], env=env, check=True, capture_output=True, timeout=45)
    api = launch('serve')
    deadline = time.monotonic() + 60
    while True:
        try:
            request(tail='/not-a-job')
        except urllib.error.HTTPError:
            break
        except urllib.error.URLError:
            assert api.poll() is None, 'API exited'
            assert time.monotonic() < deadline, 'API readiness deadline exceeded'
            time.sleep(.2)
    launch('worker'); launch('worker')
    for stage_index, payload in enumerate(payloads):
        stage = {'serialized_job_bytes': payload, 'offered_jobs': int(args.rate * args.seconds),
                 'offered_seconds': args.seconds, 'accepted': 0, 'errors': [], 'client_overload': 0}
        report['stages'].append(stage)
        prefix = f'stage-{stage_index}-'
        started = time.monotonic()
        accepted, timings = [], []
        def submit(index):
            before = time.monotonic()
            try:
                body = {'invoice': prefix + str(index), 'mode': 'normal', 'padding': ''}
                overhead = len(json.dumps(body, separators=(',', ':')).encode())
                body['padding'] = 'x' * (payload - overhead)
                value = request('POST', body=body)
                return value['id'], (time.monotonic() - before) * 1000, None
            except Exception as error:
                return None, (time.monotonic() - before) * 1000, type(error).__name__ + ': ' + str(error)
        with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
            pending = []
            for index in range(stage['offered_jobs']):
                delay = started + index / args.rate - time.monotonic()
                if delay > 0:
                    time.sleep(delay)
                # Bound in-flight requests without silently converting the load into
                # a slower closed-loop benchmark. Rejected client slots count as failures.
                if sum(not f.done() for f in pending) >= 16:
                    stage['client_overload'] += 1
                else:
                    pending.append(pool.submit(submit, index))
            for future in pending:
                job, elapsed, error = future.result()
                timings.append(elapsed)
                if error:
                    stage['errors'].append(error)
                else:
                    accepted.append(job)
        stage['accepted'] = len(accepted)
        deadline = time.monotonic() + args.drain_seconds
        rows = []
        while True:
            rows = [row for row in inspect() if row['invoice'].startswith(prefix) and row['worker']]
            if len(rows) >= len(accepted) or time.monotonic() >= deadline:
                break
            time.sleep(1)
        effects_elapsed = time.monotonic() - started
        while True:
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                states = list(pool.map(lambda job: request(tail='/' + job), accepted))
            if all(state['status'] in ('completed', 'failed', 'canceled') for state in states):
                break
            if time.monotonic() >= deadline:
                break
            time.sleep(.2)
        terminal_observed_seconds = time.monotonic() - started
        stage.update(terminal_observed_seconds=round(terminal_observed_seconds, 3), effects=len(rows), terminal_completed=sum(s['status'] == 'completed' for s in states),
                     elapsed_seconds=round(effects_elapsed, 3),
                     duplicate_execution_attempts=sum(max(0, row['attempts'] - 1) for row in rows))
        timings.sort()
        stage['enqueue_p95_ms'] = round(timings[max(0, math.ceil(len(timings) * .95) - 1)], 2) if timings else None
        stage['effective_jobs_per_second'] = round(len(rows) / stage['elapsed_seconds'], 3)
        stage['passed'] = (len(rows) == stage['offered_jobs'] == stage['terminal_completed']
                           and not stage['errors'] and not stage['client_overload']
                           and stage['terminal_observed_seconds'] <= args.seconds + 5)
        print(json.dumps(stage), flush=True)
        if not stage['passed']:
            # Do not keep growing a failing hosted ledger or exhaust a free service.
            break
    report['passed'] = len(report['stages']) == len(payloads) and all(s['passed'] for s in report['stages'])
except Exception as error:
    report.update(passed=False, error=type(error).__name__ + ': ' + str(error))
    raise
finally:
    for child in children:
        if child.poll() is None:
            child.send_signal(signal.SIGINT)
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill(); child.wait()
    for log in logs:
        log.close()
    (output / f'{tenant}.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report), flush=True)
if not report['passed']:
    raise SystemExit(1)
