#!/usr/bin/env python3
"""Fail if a tracked file names a real user's home directory.

This repository is public. Benchmark scripts, build logs and evidence have
carried absolute paths like /mnt/c/Users/<name>/... and they had to be
scrubbed out of published history. A path under a home directory is
accepted only when its user name is a placeholder or a well-known account
(CI runners, E2B's sandbox user); anything else fails, with the file and line.

Usage: check-no-local-paths.py [--self-test]
"""
import re
import subprocess
import sys

# Placeholder and well-known account names.
ALLOWED = {
    'user', 'username', 'you', 'me', 'name', 'runner', 'test', 'ubuntu', 'root',
    'admin', 'alice', 'bob', 'example', 'someone', 'default', 'public', 'shared',
    'all', 'guest',
}
PATTERNS = [
    re.compile(r'(?i)[\\/]Users[\\/]+([A-Za-z0-9._-]+)'),
    re.compile(r'/home/([a-z_][a-z0-9_-]*)'),
    re.compile(r'(?i)C--Users-([A-Za-z0-9._]+)-'),
]


def findings(text):
    for lineno, line in enumerate(text.splitlines(), 1):
        for pattern in PATTERNS:
            for m in pattern.finditer(line):
                name = m.group(1)
                if name.lower() in ALLOWED or name.startswith('<') or name.startswith('$'):
                    continue
                yield lineno, m.group(0)


def self_test():
    bad = ['/mnt/c/Users/jdoe/dev/x', r'C:\Users\jdoe\x', r'C:\\Users\\jdoe\\x',
           '/home/jdoe/.cargo', 'C--Users-jdoe-dev-x']
    good = ['/mnt/c/Users/user/dev', r'C:\Users\user\x', '/home/user/app',
            '/home/runner/work', 'C:/Users/<you>/x', '$HOME/x']
    assert all(list(findings(b)) for b in bad), bad
    assert not any(list(findings(g)) for g in good), [g for g in good if list(findings(g))]
    print('self-test passed')


def main():
    if sys.argv[1:] == ['--self-test']:
        return self_test()
    files = subprocess.run(['git', 'ls-files', '-z'], capture_output=True,
                           check=True).stdout.decode().split('\0')
    bad = 0
    for path in filter(None, files):
        if path == 'tools/check-no-local-paths.py':
            continue  # its own self-test fixtures
        try:
            data = open(path, 'rb').read()
        except OSError:
            continue
        if b'\0' in data[:8192]:
            # Binary: look for the raw bytes of a non-placeholder home path.
            text = data.decode('latin-1')
        else:
            text = data.decode('utf-8', errors='replace')
        for lineno, hit in findings(text):
            print(f'{path}:{lineno}: {hit}')
            bad += 1
    if bad:
        print(f'\n{bad} local home path(s) in tracked files. Replace the user name '
              'with a placeholder such as Users/user; this repository is public.')
        return 1
    print('no local home paths in tracked files')
    return 0


if __name__ == '__main__':
    sys.exit(main())
