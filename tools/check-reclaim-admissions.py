#!/usr/bin/env python3
"""Exercise non-overlap validation using real guarded-worker trace logs."""
import argparse
import importlib.util
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("check", Path(__file__).with_name("check-cold-start-limit.py"))
    check = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(check)
    raw = args.log.read_text()
    check.reclaim_admissions(raw)
    cases = {
        "cold admission during reclaim": "heap reclamation admitted\ncold boot admitted vm=sbx-test\n" + raw,
        "reclaim during cold admission": "cold boot admitted vm=sbx-test\nheap reclamation admitted\n" + raw,
        "unmatched cold release": "cold boot admission released vm=sbx-test\n" + raw,
        "unmatched reclaim release": "heap reclamation released\n" + raw,
        "duplicate reclaim admission": "heap reclamation admitted\nheap reclamation admitted\n" + raw,
        "missing worker summary": raw.replace("HV2_HEAP_RECLAIM_EXPERIMENT", "summary-hidden"),
    }
    for name, invalid in cases.items():
        try:
            check.reclaim_admissions(invalid)
        except ValueError:
            print(name + ": correctly refused")
        else:
            raise ValueError("accepted invalid trace: " + name)
    print("6 invalid traces refused; real non-overlapping trace accepted")


if __name__ == "__main__":
    main()
