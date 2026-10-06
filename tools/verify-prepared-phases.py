#!/usr/bin/env python3
"""Verify frozen prepared latency phase diagnostics and their base benchmark."""
import argparse
import importlib.util
import json
from pathlib import Path


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def verify(root):
    root=root.resolve();manifest=json.loads((root/'manifest.json').read_text())
    base=load('frozen_contract_verifier',root/'verify-prepared-contract.py').verify(root)
    analyzer=load('frozen_phase_analyzer',root/'analyze-prepared-phases.py')
    for cohort in manifest['cohorts']:
        result=analyzer.analyze(json.loads((root/cohort['report']).read_text()),root)
        if result!=json.loads((root/cohort['phase_analysis']).read_text()):raise ValueError('phase analysis differs')
    return {**base,'phase_accounting_verified':True,'server_internal_cause_established':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('archive',type=Path);args=parser.parse_args();print(json.dumps(verify(args.archive),indent=2))
