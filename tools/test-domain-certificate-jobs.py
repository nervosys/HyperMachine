#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import MagicMock
from unittest.mock import patch
import http.server
import ssl
import subprocess
import tempfile
import threading
import time
import multiprocessing
import os
import json
import sys

spec = importlib.util.spec_from_file_location('planner', Path(__file__).with_name('domain-certificate-jobs.py'))
planner = importlib.util.module_from_spec(spec);spec.loader.exec_module(planner)


class Jobs(unittest.TestCase):
    def setUp(self):
        self.config = {'certbot': '/owned/certbot', 'config_dir': '/owned/accounts',
                       'work_dir': '/owned/work', 'logs_dir': '/owned/logs'}
        self.template = {'manifest': '/owned/bundle', 'generations': '/owned/generations',
            'control_plane': '/owned/control', 'initial_issuance': {'webroot': '/owned/webroot',
                'email': 'operator@example.test', 'agree_tos': True}}
    def binding(self, domain, owner='sandbox-1'):
        return {'domain': domain, 'sandbox_id': owner, 'port': 8080}
    def plan(self, bindings):
        return planner.jobs(self.config, self.template, ['example.test'], bindings)
    def test_exact_suffix_boundary_and_stable_identity(self):
        records = [self.binding('evil-example.test'), self.binding('app.example.test'), self.binding('example.test.evil')]
        result = self.plan(records)
        self.assertEqual([j['domains'] for j in result], [['app.example.test']])
        self.assertEqual(result, self.plan(list(reversed(records))))
        self.assertTrue(result[0]['lineage'].startswith('/owned/accounts/live/domain-'))
    def test_guarded_jobs_revalidate_owner_and_port_and_require_credential(self):
        binding = self.binding('app.example.test')
        job = planner.jobs(self.config, self.template, ['example.test'], [binding], 'https://localhost')[0]
        with patch.dict(planner.worker.os.environ, {'HV2_DOMAIN_DISCOVERY_API_KEY': 'owned-key'}):
            planner.worker.verify_claim(job, lambda *args: [binding])
            for rows in ([], [dict(binding, sandbox_id='sandbox-2')], [dict(binding, port=3000)]):
                with self.assertRaises(ValueError): planner.worker.verify_claim(job, lambda *args: rows)
        with patch.dict(planner.worker.os.environ, {}, clear=True):
            with self.assertRaises(ValueError): planner.worker.verify_claim(job, lambda *args: [binding])
    def test_stale_claim_prevents_issuer_and_activation_commands(self):
        job = planner.jobs(self.config, self.template, ['example.test'], [self.binding('app.example.test')], 'https://localhost')[0]
        with patch.object(planner.worker, 'verify_claim', side_effect=ValueError('stale')), patch.object(planner.worker, 'command') as command:
            with self.assertRaises(ValueError): planner.worker.renew(self.config, job)
            with self.assertRaises(ValueError): planner.worker.activate(self.config, job)
            command.assert_not_called()
    def test_reconcile_adds_claims_and_retires_completed_schedule_only(self):
        binding = self.binding('app.example.test'); state = planner.worker.new_state()
        added = planner.reconcile(self.config, self.template, ['example.test'], [binding], state, 'https://localhost')
        self.assertEqual(len(added['configuration']['jobs']), 1)
        removed = planner.reconcile(added['configuration'], self.template, ['example.test'], [], state, 'https://localhost')
        self.assertTrue(removed['idle']); self.assertFalse(removed['certificate_removal_performed'])
        self.assertEqual(removed['retired_job_ids'], [added['configuration']['jobs'][0]['id']])
    def test_pending_discovered_claim_cannot_be_removed_or_rebound(self):
        binding = self.binding('app.example.test'); state = planner.worker.new_state()
        config = planner.reconcile(self.config, self.template, ['example.test'], [binding], state, 'https://localhost')['configuration']
        job = config['jobs'][0]
        global_config = {k: v for k, v in config.items() if k != 'jobs'}
        state['jobs'][job['id']] = {'configuration_sha256': planner.worker.fingerprint({'worker': global_config, 'job': job}),
            'next_check_at': 0, 'last_started_at': 0, 'pending': {'phase': 'deploying', 'renewal_exit': 0}, 'last_result': None}
        planner.reconcile(config, self.template, ['example.test'], [binding], state, 'https://localhost')
        for records in ([], [dict(binding, sandbox_id='sandbox-2')], [dict(binding, port=3000)]):
            with self.assertRaises(ValueError): planner.reconcile(config, self.template, ['example.test'], records, state, 'https://localhost')
        self.assertEqual(state['jobs'][job['id']]['pending']['phase'], 'deploying')
    def test_managed_identity_is_persisted_before_execution_and_not_aliased(self):
        binding = self.binding('app.example.test'); state = planner.worker.new_state()
        config = planner.reconcile(self.config, self.template, ['example.test'], [binding], state, 'https://localhost')['configuration']
        saved = []
        planner.worker.run_cycle(config, state, lambda value: saved.append(json.loads(json.dumps(value))), now=1000,
            renew_fn=lambda *args: 0, activate_fn=lambda *args: {'activation_verified': True, 'active_leaf_sha256': 'a' * 64})
        identity = config['jobs'][0]['id']
        self.assertIn('managed_job', saved[0]['jobs'][identity])
        planner.worker.validate_state(state)
        config['jobs'][0]['domain_claim']['sandbox_id'] = 'other-owner'
        self.assertEqual(state['jobs'][identity]['managed_job']['domain_claim']['sandbox_id'], 'sandbox-1')
    def test_journal_rejects_managed_identity_with_unknown_or_wrong_job_fields(self):
        state = planner.worker.new_state()
        config = planner.reconcile(self.config, self.template, ['example.test'], [self.binding('app.example.test')], state, 'https://localhost')['configuration']
        planner.worker.run_cycle(config, state, lambda value: None, now=1000, renew_fn=lambda *args: 0,
            activate_fn=lambda *args: {'activation_verified': True, 'active_leaf_sha256': 'a' * 64})
        entry = state['jobs'][config['jobs'][0]['id']]
        entry['managed_job']['extra'] = 'untrusted'
        with self.assertRaises(ValueError): planner.worker.validate_state(state)
    def test_retirement_requires_absent_claim_and_verified_unchanged_target(self):
        state = planner.worker.new_state(); binding = self.binding('app.example.test')
        config = planner.reconcile(self.config, self.template, ['example.test'], [binding], state, 'https://localhost')['configuration']
        planner.worker.run_cycle(config, state, lambda value: None, now=1000, renew_fn=lambda *args: 0,
            activate_fn=lambda *args: {'activation_verified': True, 'active_leaf_sha256': 'a' * 64})
        settings = {'origin': 'https://localhost', 'template': self.template}
        desired = dict(config, jobs=[])
        candidates = planner.retirement_candidates(config, desired, [], state, settings)
        self.assertEqual(candidates[0]['expected_leaf_sha256'], 'a' * 64)
        before = planner.worker.canonical(state)
        with patch.dict(os.environ, HV2_DOMAIN_DISCOVERY_API_KEY='owned-key'):
            with self.assertRaisesRegex(ValueError, 'journal retained'):
                planner.worker.discovered_configuration(config, state,
                    dict(settings, allowed_suffixes=['example.test']), collect=lambda *args: [])
        self.assertEqual(planner.worker.canonical(state), before)
        with self.assertRaises(ValueError): planner.retirement_candidates(config, desired, [binding], state, settings)
        changed = dict(settings, template=dict(self.template, manifest='/other/bundle'))
        with self.assertRaises(ValueError): planner.retirement_candidates(config, desired, [], state, changed)
        entry = state['jobs'][config['jobs'][0]['id']]; del entry['managed_job']
        with self.assertRaises(ValueError): planner.retirement_candidates(config, desired, [], state, settings)
    def test_conflicting_claims_refused(self):
        with self.assertRaises(ValueError): self.plan([self.binding('app.example.test'), self.binding('app.example.test', 'sandbox-2')])
    def test_no_initial_authority_without_explicit_template(self):
        del self.template['initial_issuance']
        with self.assertRaises(ValueError): self.plan([self.binding('app.example.test')])
    def test_capacity_refusal_preserves_inventory(self):
        with self.assertRaises(ValueError): self.plan([self.binding(f'a{i}.example.test') for i in range(65)])
    def test_empty_eligible_inventory_returns_no_jobs(self):
        self.assertEqual(self.plan([self.binding('outside.test')]), [])
    def test_invalid_binding_and_path_injection_refused(self):
        for record in (self.binding('../outside.test'), self.binding('app.example.test', '../owner'),
                       dict(self.binding('app.example.test'), extra=True)):
            with self.assertRaises(ValueError): self.plan([record])
    def test_inventory_preserves_owner_and_refuses_changes(self):
        reads = {'/sandboxes': [{'sandboxID': 'sandbox-1'}],
                 '/sandboxes/sandbox-1/domains': [self.binding('app.example.test')]}
        self.assertEqual(planner.inventory(reads.__getitem__), reads['/sandboxes/sandbox-1/domains'])
        count = [0]
        def changed(path):
            if path.endswith('/domains'):
                count[0] += 1
                return [self.binding('app.example.test', 'sandbox-1' if count[0] == 1 else 'sandbox-2')]
            return reads[path]
        with self.assertRaises(ValueError): planner.inventory(changed)
    def test_inventory_refuses_duplicate_ids_and_wrong_owners(self):
        for reads in ({'/sandboxes': [{'sandboxID': 'sandbox-1'}] * 2},
                      {'/sandboxes': [{'sandboxID': 'sandbox-1'}],
                       '/sandboxes/sandbox-1/domains': [self.binding('app.example.test', 'sandbox-2')]}):
            with self.assertRaises(ValueError): planner.inventory(reads.__getitem__)
    def test_reader_refuses_untrusted_origin_and_credential_injection(self):
        for origin in ('http://localhost', 'https://user@localhost', 'https://localhost/path', 'https://localhost?token=x'):
            with self.assertRaises(ValueError): planner.HTTPSReader(origin, 'owned-key')
        with self.assertRaises(ValueError): planner.HTTPSReader('https://localhost', 'key\nInjected: value')
    def test_reader_bounds_response_and_authenticates_only_supported_paths(self):
        reader = planner.HTTPSReader('https://localhost', 'owned-key')
        response = MagicMock(); response.status = 200; response.read.return_value = b'[]'
        response.__enter__.return_value = response
        reader._opener = MagicMock(); reader._opener.open.return_value = response
        self.assertEqual(reader('/sandboxes'), [])
        request = reader._opener.open.call_args.args[0]
        self.assertEqual(request.get_header('X-api-key'), 'owned-key')
        self.assertEqual(reader._opener.open.call_args.kwargs['timeout'], 5)
        with self.assertRaises(ValueError): reader('//outside.test')
        response.read.return_value = b'x' * (1024 * 1024 + 1)
        with self.assertRaises(ValueError): reader('/sandboxes')
        response.read.return_value = b'{"domain":1,"domain":2}'
        with self.assertRaises(ValueError): reader('/sandboxes')
    def test_redirect_is_refused(self):
        with self.assertRaises(ValueError): planner.NoRedirect().redirect_request(None, None, None, None, None, None)
    def test_whole_cycle_deadline_stops_stalled_owned_reader(self):
        before = {p.pid for p in multiprocessing.active_children()}
        def stalled(*args):
            time.sleep(60)
        started = time.monotonic()
        with self.assertRaises(ValueError):
            planner.bounded_inventory('https://localhost', 'owned-key', timeout=.1, reader_factory=stalled)
        self.assertLess(time.monotonic() - started, 4)
        self.assertEqual({p.pid for p in multiprocessing.active_children()}, before)
    def test_owned_inventory_process_returns_records_and_redacts_failure(self):
        def factory(*args):
            return lambda path: [{'sandboxID': 'sandbox-1'}] if path == '/sandboxes' else [self.binding('app.example.test')]
        self.assertEqual(planner.bounded_inventory('https://localhost', 'owned-key', reader_factory=factory),
                         [self.binding('app.example.test')])
        def failed(*args): raise ValueError('owned-secret-must-not-escape')
        with self.assertRaisesRegex(ValueError, '^authenticated inventory collection failed$'):
            planner.bounded_inventory('https://localhost', 'owned-key', reader_factory=failed)
    def test_real_https_reader_checks_ca_and_api_key(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch); cert = root / 'cert.pem'; key = root / 'key.pem'
            subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
                '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost',
                '-keyout', str(key), '-out', str(cert)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            class Handler(http.server.BaseHTTPRequestHandler):
                def log_message(self, *args): pass
                def do_GET(self):
                    valid = self.headers.get('x-api-key') == 'owned-key'
                    body = b'[]'
                    self.send_response(200 if valid else 403)
                    self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body)
            server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); context.load_cert_chain(cert, key)
            server.socket = context.wrap_socket(server.socket, server_side=True)
            thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
            try:
                origin = 'https://localhost:' + str(server.server_port)
                self.assertEqual(planner.HTTPSReader(origin, 'owned-key', cert)('/sandboxes'), [])
                with self.assertRaises(ValueError): planner.HTTPSReader(origin, 'wrong-key', cert)('/sandboxes')
                with self.assertRaises(ValueError): planner.HTTPSReader(origin, 'owned-key')('/sandboxes')
                config = root / 'worker.json'; settings = root / 'discovery.json'
                journal = root / 'journal'; journal.mkdir(mode=0o700)
                config.write_text(json.dumps(dict(self.config, jobs=[])))
                settings.write_text(json.dumps({'origin': origin, 'ca_file': str(cert),
                    'allowed_suffixes': ['example.test'], 'template': self.template}))
                result = subprocess.run([sys.executable, str(Path(__file__).with_name('renew-tls-certificates.py')),
                    '--config', str(config), '--state', str(journal / 'state.json'), '--discovery-config', str(settings)],
                    env=dict(os.environ, HV2_DOMAIN_DISCOVERY_API_KEY='owned-key'), capture_output=True, timeout=15, check=True)
                self.assertEqual(json.loads(result.stdout), {'jobs': []})
                self.assertEqual(json.loads((journal / 'state.json').read_bytes()), {'version': 1, 'jobs': {}})
                self.assertNotIn('owned-key', (journal / 'state.json').read_text())
            finally:
                server.shutdown(); server.server_close(); thread.join(timeout=5)
                self.assertFalse(thread.is_alive())


if __name__ == '__main__': unittest.main()
