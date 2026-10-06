#!/usr/bin/env python3
"""Read owned-process smaps without modifying its memory or allocator."""
from pathlib import Path
import re
import time

FIELDS = ('Size','Rss','Pss','Shared_Clean','Shared_Dirty','Private_Clean','Private_Dirty','Anonymous','AnonHugePages','Swap')


def parse(text):
    result, current = [], None
    for line in text.splitlines():
        if re.match(r'^[0-9a-f]+-[0-9a-f]+ ', line):
            fields = line.split(maxsplit=5)
            current = {'range':fields[0], 'permissions':fields[1], 'offset':fields[2], 'path':fields[5] if len(fields)>5 else ''}
            result.append(current)
        elif current is not None:
            key, separator, value = line.partition(':')
            if separator and key in FIELDS:
                tokens = value.split()
                if len(tokens)!=2 or tokens[1]!='kB' or not tokens[0].isdigit():
                    raise ValueError('invalid smaps field: '+key)
                current[key+'_kib'] = int(tokens[0])
    if not result or any(any(k+'_kib' not in m for k in FIELDS) for m in result):
        raise ValueError('incomplete smaps mappings')
    groups = {}
    for mapping in result:
        path = mapping['path']
        category = ('guest_sized' if mapping['Size_kib']>=1024*1024 else
                    'heap' if path=='[heap]' else 'anonymous' if not path or path.startswith('[anon') else
                    'stack' if path.startswith('[stack') else 'special' if path.startswith('[') else 'file')
        group = groups.setdefault(category, {'count':0, **{k+'_kib':0 for k in FIELDS}})
        group['count'] += 1
        for field in FIELDS:
            group[field+'_kib'] += mapping[field+'_kib']
    return {'groups':groups, 'guest_sized_mappings':[m for m in result if m['Size_kib']>=1024*1024], 'mapping_count':len(result)}


def observe(pid):
    started = time.monotonic_ns()
    with Path(f'/proc/{pid}/smaps').open() as stream:
        text = stream.read(16*1024*1024+1)
    if len(text)>16*1024*1024:
        raise ValueError('owned smaps exceeds diagnostic limit')
    return {'pid':pid, 'started_monotonic_ns':started, 'read_duration_ns':time.monotonic_ns()-started, 'raw_smaps':text, **parse(text)}
