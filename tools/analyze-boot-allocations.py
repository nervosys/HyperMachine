#!/usr/bin/env python3
"""Summarize adjacent boot allocation observations, without PSS attribution."""
import argparse
import json
from pathlib import Path


def analyze(path):
    report = json.loads(path.read_text())
    if not report['success'] or not report['diagnostic_only']:
        raise ValueError('requires successful diagnostic cohort')
    events = report['events']
    if [row['seq'] for row in events] != list(range(len(events))):
        raise ValueError('missing or reordered observations')
    highest = []
    for index, row in enumerate(events):
        if row['stage'] == 'highest-live':
            following = events[index + 1]
            if following['stage'] != 'highest-dropped' or following['bytes'] != row['bytes']:
                raise ValueError('unmatched highest-address drop')
            highest.append({'region_bytes': row['bytes'],
                            'used_bytes_released': row['used'] - following['used'],
                            'free_bytes_increase': following['free'] - row['free'],
                            'arena_bytes_change': following['arena'] - row['arena'],
                            'mapped_bytes_change': following['mapped'] - row['mapped']})
    copies = []
    for before, after in zip(events, events[1:]):
        for label in ('initrd', 'kernel'):
            if before['stage'] == label + '-copy-before' and after['stage'] == label + '-copy-live':
                copies.append({'image': label, 'payload_bytes': after['bytes'],
                               'used_bytes_increase': after['used'] - before['used']})
    if len(highest) != 1 or len(copies) != 4:
        raise ValueError('unexpected bounded boot observation profile')
    return {'report': path.name, 'highest_address': highest, 'image_copies': copies,
            'guest_checks_passed': True, 'owned_node_exit_code': report['owned_node_exit_code']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path, nargs='+')
    args = parser.parse_args()
    print(json.dumps({'diagnostic_only': True, 'performance_win_established': False,
                      'runtime_change_adopted': False, 'runs': [analyze(p) for p in args.reports],
                      'limitations': ['allocator transitions do not establish retained PSS ownership',
                                      'concurrent process allocations and diagnostic logging can confound deltas']}, indent=2))
