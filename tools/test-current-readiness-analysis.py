#!/usr/bin/env python3
"""Replay archived readiness evidence and reject damaged comparison contracts."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('analysis', ROOT / 'tools/analyze-current-readiness.py')
analysis = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analysis)


class Contract(unittest.TestCase):
    def setUp(self):
        self.folder = ROOT / 'docs/benchmarks/2026-10-02/current-readiness'
        self.baseline = self.folder.parent / 'current-native-engines/c100.json'
        self.diagnostic = self.folder / 'c100.json'

    def test_preserved_results_unchanged(self):
        expected = json.loads((self.folder / 'analysis.json').read_text(encoding='utf-8'))
        self.assertEqual(analysis.analyze(self.baseline, self.diagnostic), expected)

    def test_damaged_contracts_are_rejected(self):
        original = json.loads(self.diagnostic.read_text(encoding='utf-8'))
        mutations = []
        for field in ('diagnostic_only', 'artifacts_unchanged', 'cold_ids_match_passed_requests', 'stage_ids_match_passed_requests'):
            for value in (False, 1):
                mutations.append((field, value))
        mutations.append(('concurrency', 99))
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'damaged.json'
            for field, value in mutations:
                with self.subTest(field=field, value=value):
                    report = copy.deepcopy(original); report[field] = value
                    path.write_text(json.dumps(report), encoding='utf-8')
                    with self.assertRaises(ValueError): analysis.analyze(self.baseline, path)
            for name in ('hypermachine', 'firecracker', 'kernel', 'initrd'):
                with self.subTest(artifact=name):
                    report = copy.deepcopy(original); report['artifact_sha256'][name] = '0' * 64
                    path.write_text(json.dumps(report), encoding='utf-8')
                    with self.assertRaises(ValueError): analysis.analyze(self.baseline, path)

    def test_raw_identity_and_duration_corruption_is_rejected(self):
        original = json.loads(self.diagnostic.read_text(encoding='utf-8'))
        cases = []
        for field in ('cold_readiness_stages_ms', 'startup_stages_ms'):
            report = copy.deepcopy(original)
            identity = next(iter(report[field]))
            report[field]['unrelated-guest'] = report[field].pop(identity)
            cases.append(report)
        report = copy.deepcopy(original)
        rows = next(batch['samples'] for batch in report['batches'] if batch['engine'] == 'hypermachine')
        rows[1]['sandbox_id'] = rows[0]['sandbox_id']; cases.append(report)
        for value in (-1, float('nan'), float('inf'), True):
            report = copy.deepcopy(original)
            next(iter(report['cold_readiness_stages_ms'].values()))['connect_ms'] = value
            cases.append(report)
        report = copy.deepcopy(original)
        next(iter(report['cold_readiness_stages_ms'].values()))['succeeded'] = False
        cases.append(report)
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'damaged.json'
            for index, report in enumerate(cases):
                with self.subTest(case=index):
                    path.write_text(json.dumps(report), encoding='utf-8')
                    with self.assertRaises(ValueError): analysis.analyze(self.baseline, path)


if __name__ == '__main__':
    unittest.main()
