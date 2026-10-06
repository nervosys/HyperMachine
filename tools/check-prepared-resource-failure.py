#!/usr/bin/env python3
"""Run real owned guests but falsify only post-timing resource GET responses."""
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location('resource_failure_coordinator', Path(__file__).with_name('bench-prepared-engines.py'))
coordinator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coordinator)
request = coordinator.engines.request
fapi = coordinator.fapi
injected = {'hypermachine': 0, 'firecracker': 0}

def bad_request(url, method, route, *args, **kwargs):
    value = request(url, method, route, *args, **kwargs)
    if method == 'GET' and route.startswith('/sandboxes/'):
        value = dict(value, memoryMB=512)
        injected['hypermachine'] += 1
    return value

def bad_fapi(path, method, route, *args, **kwargs):
    value = fapi(path, method, route, *args, **kwargs)
    if method == 'GET' and route == '/machine-config':
        value = dict(value, mem_size_mib=512)
        injected['firecracker'] += 1
    return value

if __name__ == '__main__':
    coordinator.engines.request = bad_request
    coordinator.fapi = bad_fapi
    result = coordinator.main()
    if result != 1 or any(v == 0 for v in injected.values()):
        raise ValueError('resource mismatch did not fail both engines')
    # main writes its real cleanup and per-sample errors to the requested output.
    print(json.dumps({'fault_injection_only': True, 'resource_gets_falsified': injected,
                      'benchmark_failure_required': True, 'performance_win_established': False}))
