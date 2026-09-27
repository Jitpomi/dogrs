"""Fail closed on broken experiments; distinguish complete capacity misses."""
import math


def validate_result(measurement, returncode):
    if not isinstance(measurement, dict):
        raise ValueError('Missing diagnostic measurement')
    expected = {'offered': 60000, 'payload_bytes': 65536, 'tenants': 100,
                'seconds': 60, 'jobs_per_second_per_tenant': 10,
                'workers_per_tenant': 8, 'max_inflight_per_tenant': 32}
    for key, value in expected.items():
        if type(measurement.get(key)) is not int or measurement[key] != value:
            raise ValueError(f'Unexpected workload field: {key}')
    for key in ('accepted', 'completed', 'verified_terminal_once', 'overload', 'late_offers', 'error_count'):
        if type(measurement.get(key)) is not int or measurement[key] < 0:
            raise ValueError(f'Invalid diagnostic counter: {key}')
    if measurement['error_count'] or measurement.get('errors') != []:
        raise ValueError('Diagnostic encountered operation errors')
    if not (measurement['accepted'] == measurement['completed'] == measurement['verified_terminal_once']):
        raise ValueError('Accepted jobs were not all completed and verified')
    if measurement['accepted'] + measurement['overload'] != measurement['offered']:
        raise ValueError('Diagnostic did not account for every offered job')
    if measurement['late_offers'] > measurement['offered']:
        raise ValueError('Invalid late-offer count')
    if (measurement.get('shards') != '16'
            or measurement.get('nats_connections') != 'per-shard'
            or measurement.get('payload_pattern') != 'unique-per-tenant-and-sequence'
            or measurement.get('latency_includes_recovery') is not False
            or measurement.get('overload_recovery') is not None):
        raise ValueError('Diagnostic changed the required measurement profile')
    elapsed = measurement.get('elapsed_seconds')
    if type(elapsed) not in (int, float) or not math.isfinite(elapsed) or elapsed < 0:
        raise ValueError('Invalid diagnostic elapsed time')
    passed = measurement['accepted'] == 60000 and measurement['late_offers'] == 0 and elapsed <= 65
    if type(measurement.get('passed')) is not bool or measurement['passed'] != passed:
        raise ValueError('Diagnostic verdict contradicts its counters')
    if returncode != (0 if passed else 1):
        raise ValueError('Unexpected diagnostic process exit status')
    return passed
