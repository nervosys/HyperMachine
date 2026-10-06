#!/usr/bin/env python3
"""Check explicit issuance authority and refusal to replace interrupted state."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('initial_worker', Path(__file__).with_name('renew-tls-certificates.py'))
worker = importlib.util.module_from_spec(spec); spec.loader.exec_module(worker)


class InitialIssuance(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name); (self.root / 'webroot').mkdir(mode=0o700)
        self.raw = {'certbot': '/owned/certbot', 'config_dir': str(self.root / 'account'),
            'work_dir': str(self.root / 'work'), 'logs_dir': str(self.root / 'logs'),
            'jobs': [{'id': 'app', 'cert_name': 'app', 'lineage': str(self.root / 'account/live/app'),
                'manifest': '/owned/bundle.json', 'generations': '/owned/generations', 'control_plane': '/owned/control',
                'domains': ['app.example.test'], 'initial_issuance': {'webroot': str(self.root / 'webroot'),
                    'email': 'operator@example.test', 'agree_tos': True}}]}
        self.commands = []
    def command(self, argv, *args, **kwargs):
        self.commands.append(argv)
        return 0, json.dumps({'success': True, 'activation_verified': True, 'active_leaf_sha256': 'a' * 64}).encode()
    def renew(self):
        config = worker.configuration(self.raw)
        with patch.object(worker, 'command', self.command):
            worker.renew(config, config['jobs'][0], force=True)
        return self.commands[-1]
    def test_new_job_issues_only_named_domains_with_explicit_terms(self):
        argv = self.renew()
        self.assertEqual(argv[1], 'certonly'); self.assertIn('--agree-tos', argv)
        self.assertEqual(argv[argv.index('--domain') + 1], 'app.example.test')
        self.assertNotIn('--force-renewal', argv)
    def test_saved_renewal_state_is_never_reissued_as_new(self):
        path = self.root / 'account/renewal/app.conf'; path.parent.mkdir(parents=True); path.write_text('owned state')
        self.assertEqual(self.renew()[1], 'renew')
    def test_dangling_lineage_is_not_new(self):
        path = self.root / 'account/live/app'; path.parent.mkdir(parents=True); path.symlink_to(self.root / 'missing')
        self.assertEqual(self.renew()[1], 'renew')
    def test_existing_jobs_do_not_gain_initial_authority(self):
        del self.raw['jobs'][0]['initial_issuance']
        self.assertEqual(self.renew()[1], 'renew')
    def test_shared_writable_webroot_refused(self):
        (self.root / 'webroot').chmod(0o777)
        with self.assertRaises(ValueError): self.renew()
        self.assertEqual(self.commands, [])
    def test_terms_must_be_explicit_boolean_true(self):
        for value in (False, 1, 'true'):
            raw = copy.deepcopy(self.raw); raw['jobs'][0]['initial_issuance']['agree_tos'] = value
            with self.assertRaises(ValueError): worker.configuration(raw)
    def test_invalid_email_and_unknown_issuance_options_refused(self):
        raw = copy.deepcopy(self.raw); raw['jobs'][0]['initial_issuance']['email'] = 'operator@example.test\n--extra'
        with self.assertRaises(ValueError): worker.configuration(raw)
        self.raw['jobs'][0]['initial_issuance']['extra'] = True
        with self.assertRaises(ValueError): worker.configuration(self.raw)
    def test_activation_can_provision_only_explicit_initial_jobs(self):
        config = worker.configuration(self.raw)
        with patch.object(worker, 'selected_pid', return_value=42), patch.object(worker, 'command', self.command):
            worker.activate(config, config['jobs'][0])
        self.assertIn('--provision-new-group', self.commands[-1])


if __name__ == '__main__': unittest.main()
