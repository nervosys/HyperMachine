#!/usr/bin/env python3
"""Check new-group bounds, overlap refusal and default preservation."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('deployment', Path(__file__).with_name('deploy-tls-certificate.py'))
deployment = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deployment)


class Provisioning(unittest.TestCase):
    def bundle(self):
        return {'default': {'cert_path': '/owned/default.pem', 'key_path': '/owned/key.pem'}, 'certificates': []}

    def test_existing_mode_refuses_new_group_without_mutation(self):
        document = self.bundle(); previous = copy.deepcopy(document)
        with self.assertRaises(ValueError):
            deployment.deployment_entry(document, ['app.example.com'])
        self.assertEqual(previous, document)

    def test_new_group_preserves_default_and_copies_names(self):
        document = self.bundle(); fallback = copy.deepcopy(document['default']); names = ['app.example.com']
        entry = deployment.deployment_entry(document, names, True)
        entry['cert_path'] = '/owned/new.pem'; names.append('other.example.com')
        self.assertEqual(document['default'], fallback)
        self.assertEqual(entry['names'], ['app.example.com'])

    def test_existing_group_is_idempotent_without_provision_flag(self):
        document = self.bundle()
        first = deployment.deployment_entry(document, ['app.example.com'], True)
        self.assertIs(deployment.deployment_entry(document, ['app.example.com']), first)
        self.assertEqual(len(document['certificates']), 1)

    def test_overlap_refused_without_mutation(self):
        document = self.bundle()
        deployment.deployment_entry(document, ['app.example.com', 'other.example.com'], True)
        previous = copy.deepcopy(document)
        with self.assertRaises(ValueError):
            deployment.deployment_entry(document, ['app.example.com'], True)
        self.assertEqual(previous, document)

    def test_missing_default_refused(self):
        with self.assertRaises(ValueError):
            deployment.deployment_entry({'certificates': []}, ['app.example.com'], True)

    def test_group_limit(self):
        document = self.bundle()
        document['certificates'] = [{'names': [f'a{i}.example.com']} for i in range(128)]
        with self.assertRaises(ValueError):
            deployment.deployment_entry(document, ['app.example.com'], True)
        self.assertEqual(len(document['certificates']), 128)

    def test_alias_limit(self):
        document = self.bundle()
        document['certificates'] = [{'names': [f'a{i}.example.com' for i in range(1024)]}]
        with self.assertRaises(ValueError):
            deployment.deployment_entry(document, ['app.example.com'], True)
        self.assertEqual(len(document['certificates']), 1)

    def test_case_normalized_overlap(self):
        document = self.bundle()
        document['certificates'] = [{'names': ['APP.example.com', 'other.example.com']}]
        with self.assertRaises(ValueError):
            deployment.deployment_entry(document, ['app.example.com'], True)


if __name__ == '__main__':
    unittest.main()
