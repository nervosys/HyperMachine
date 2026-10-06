#!/usr/bin/env python3
"""Build bounded certificate jobs from owned claims and explicit operator policy.

Pure planner; callers must supply authenticated control-plane binding records.
It performs neither API discovery nor issuance itself.
"""
import hashlib
import importlib.util
import json
import multiprocessing
import os
from pathlib import Path
import re
import ssl
import urllib.error
import urllib.request
from urllib.parse import urlsplit

spec = importlib.util.spec_from_file_location('domain_worker', Path(__file__).with_name('renew-tls-certificates.py'))
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
require = worker.require


def bounded_inventory(origin, api_key, ca_file=None, timeout=30, reader_factory=None):
    """Read inventory in an owned Linux child, including DNS in the deadline."""
    require(os.name == 'posix' and 0.1 <= timeout <= 300, 'invalid discovery process deadline')
    context = multiprocessing.get_context('fork')
    receive, send = context.Pipe(duplex=False)
    def collect():
        receive.close()
        try:
            reader = (reader_factory or HTTPSReader)(origin, api_key, ca_file)
            raw = json.dumps({'bindings': inventory(reader)}, separators=(',', ':')).encode()
            require(len(raw) <= 1024 * 1024, 'inventory result exceeds limit')
        except Exception:
            raw = b'{"error":"authenticated inventory collection failed"}'
        try:
            send.send_bytes(raw)
        finally:
            send.close()
    process = context.Process(target=collect, name='owned-domain-inventory')
    try:
        process.start(); send.close()
        require(receive.poll(timeout), 'inventory collection exceeded whole-cycle deadline')
        result = json.loads(receive.recv_bytes(1024 * 1024))
        require(isinstance(result, dict) and result.keys() == {'bindings'}, 'authenticated inventory collection failed')
        return result['bindings']
    finally:
        receive.close(); send.close()
        if process.pid is not None:
            process.join(timeout=0.1)
            if process.is_alive():
                process.terminate(); process.join(timeout=1)
            if process.is_alive():
                process.kill(); process.join(timeout=2)
            require(not process.is_alive(), 'owned inventory process termination unconfirmed')
            process.close()


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise ValueError('control-plane redirect refused')


class HTTPSReader:
    """Certificate-verified API reader with per-operation timeout and size cap.

    System DNS resolution is not bounded by urllib's socket timeout. A service
    supervisor must impose a whole-cycle deadline before unattended operation.
    """
    def __init__(self, origin, api_key, ca_file=None):
        parsed = urlsplit(origin)
        require(parsed.scheme == 'https' and parsed.hostname and not parsed.username
                and not parsed.password and parsed.path in ('', '/') and not parsed.query and not parsed.fragment,
                'control plane requires a credential-free HTTPS origin')
        _ = parsed.port
        require(isinstance(api_key, str) and 1 <= len(api_key) <= 4096
                and all(33 <= ord(c) <= 126 for c in api_key), 'invalid API credential')
        self.origin = origin.rstrip('/')
        self._key = api_key
        context = ssl.create_default_context(cafile=str(ca_file) if ca_file else None)
        self._opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect(),
                                                   urllib.request.HTTPSHandler(context=context))

    def __call__(self, path):
        require(re.fullmatch(r'/sandboxes(?:/[A-Za-z0-9_-]{1,128}/domains)?', path), 'unsupported discovery path')
        request = urllib.request.Request(self.origin + path, headers={'x-api-key': self._key, 'Accept': 'application/json'})
        try:
            with self._opener.open(request, timeout=5) as response:
                require(response.status == 200, 'control-plane response rejected')
                raw = response.read(1024 * 1024 + 1)
                require(len(raw) <= 1024 * 1024, 'control-plane response exceeds limit')
                return json.loads(raw, object_pairs_hook=worker.deployment.no_duplicate_keys)
        except (urllib.error.URLError, OSError, json.JSONDecodeError) as error:
            raise ValueError('authenticated control-plane read failed') from error


def inventory(read):
    """Collect claims through an authenticated reader; refuse changing inventory.

    `read(path)` must enforce trusted transport, authentication and response
    byte/deadline bounds. These repeated observations are not an atomic snapshot.
    """
    def identities():
        records = read('/sandboxes')
        require(isinstance(records, list) and len(records) <= 4096, 'sandbox inventory exceeds limit')
        values = [record.get('sandboxID') if isinstance(record, dict) else None for record in records]
        require(all(isinstance(value, str) and 1 <= len(value) <= 128
                    and all(c.isascii() and (c.isalnum() or c in '-_') for c in value) for value in values),
                'invalid sandbox identity')
        require(len(values) == len(set(values)), 'duplicate sandbox identity')
        return sorted(values)
    ids = identities()
    def bindings():
        result = []
        for identity in ids:
            rows = read('/sandboxes/' + identity + '/domains')
            require(isinstance(rows, list), 'invalid domain inventory')
            require(len(result) + len(rows) <= 4096, 'domain inventory exceeds limit')
            for row in rows:
                require(isinstance(row, dict) and row.keys() == {'domain', 'sandbox_id', 'port'}
                        and row['sandbox_id'] == identity, 'domain owner differs from queried sandbox')
                worker.deployment.name(row['domain'])
                worker.integer(row['port'], 1, 65535, 'guest port')
            result.extend(rows)
        require(len({worker.deployment.name(row['domain']) for row in result}) == len(result), 'conflicting domain ownership')
        return sorted(result, key=lambda row: row['domain'])
    first = bindings()
    require(ids == identities() and first == bindings(), 'domain inventory changed during discovery')
    return first


def jobs(configuration, template, allowed_suffixes, bindings, claim_origin=None, claim_ca=None):
    require(isinstance(allowed_suffixes, list) and 1 <= len(allowed_suffixes) <= 64, 'invalid operator domain policy')
    suffixes = {worker.deployment.name(value) for value in allowed_suffixes}
    require(len(suffixes) == len(allowed_suffixes), 'duplicate operator domain suffix')
    require(isinstance(bindings, list) and len(bindings) <= 4096, 'binding inventory exceeds limit')
    require(isinstance(template, dict) and 'initial_issuance' in template, 'initial issuance must be explicitly authorized')
    result, owners = [], {}
    for binding in bindings:
        require(isinstance(binding, dict) and binding.keys() == {'domain', 'sandbox_id', 'port'}, 'invalid binding record')
        domain = worker.deployment.name(binding['domain'])
        owner = binding['sandbox_id']
        require(isinstance(owner, str) and 1 <= len(owner) <= 128
                and all(c.isascii() and (c.isalnum() or c in '-_') for c in owner), 'invalid sandbox owner')
        worker.integer(binding['port'], 1, 65535, 'guest port')
        require(domain not in owners, 'duplicate or conflicting domain ownership')
        owners[domain] = owner
        if not any(domain == suffix or domain.endswith('.' + suffix) for suffix in suffixes):
            continue
        identity = 'domain-' + hashlib.sha256(domain.encode('ascii')).hexdigest()
        job = dict(template, id=identity, cert_name=identity, domains=[domain],
                   lineage=str(Path(configuration['config_dir']) / 'live' / identity))
        if claim_origin is not None:
            job['domain_claim'] = dict(binding, origin=claim_origin)
            if claim_ca is not None:
                job['domain_claim']['ca_file'] = str(claim_ca)
        result.append(job)
    require(len(result) <= 64, 'eligible domain inventory exceeds worker capacity')
    result.sort(key=lambda job: job['domains'][0])
    if result:
        worker.configuration(dict(configuration, jobs=result))
    return result


def reconcile(configuration, template, allowed_suffixes, bindings, state, origin, ca_file=None):
    """Plan a replacement worker config without editing config, journal or TLS.

    Retiring a completed job stops renewal scheduling only. Certificate removal
    requires a separate verified deployment transaction.
    """
    worker.validate_state(state)
    managed = []
    static = []
    for job in configuration.get('jobs', []):
        if re.fullmatch(r'domain-[0-9a-f]{64}', job['id']):
            require('domain_claim' in job, 'operator job uses reserved discovered identity')
            managed.append(job)
        else:
            static.append(job)
    discovered = jobs(configuration, template, allowed_suffixes, bindings, origin, ca_file)
    planned = dict(configuration, jobs=static + discovered)
    if planned['jobs']:
        planned = worker.configuration(planned)
    else:
        # No worker invocation is needed for a fully retired inventory.
        planned = dict(configuration, jobs=[])
    identities = {job['id']: job for job in planned['jobs']}
    require(len(identities) == len(planned['jobs']), 'static and discovered job identity conflict')
    global_config = {key: value for key, value in planned.items() if key != 'jobs'}
    for identity, previous in state['jobs'].items():
        if previous['pending'] is not None:
            require(identity in identities, 'discovery would remove a pending job; reconciliation required')
            expected = worker.fingerprint({'worker': global_config, 'job': identities[identity]})
            require(previous['configuration_sha256'] == expected,
                    'discovery would change a pending job; reconciliation required')
    retired = sorted(job['id'] for job in managed if job['id'] not in identities)
    return {'configuration': planned, 'retired_job_ids': retired,
            'certificate_removal_performed': False, 'idle': not planned['jobs']}
