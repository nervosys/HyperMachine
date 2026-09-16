#!/usr/bin/env python3
"""Do the API names in the documentation exist in the code?

Four documented surfaces were audited by hand and the results were not close
to each other:

    REST routes     40 of 41 correct
    CLI commands     8 of 16
    MCP tool names   7 of 12
    gRPC service     0 of 13

The gradient tracks how likely a thing is to be *run*. Routes get invoked; a
protobuf transcribed into a markdown page is executed by nothing, and it had
drifted until it shared nothing with the real service but the port number.

So the two surfaces that are pure string comparison are checked here, and this
runs in the sweep. The config pages have their own check --
`crates/hv2-api/tests/documented_config_is_real.rs` -- because the schema can
answer precisely and a heuristic is not needed.

Not covered: CLI commands (they need the built binary; `hm <cmd> --help` is
the check, and it was run by hand) and the gRPC service (one block, now
pointing at vm.proto rather than copying it, which is the fix that keeps).

Exit status is 1 only for something new: every known-good exception is in
ACCEPTED with the reason, the same convention `tools/sweep.sh` uses for SKIP.
"""

import os
import re
import sys

# ---------------------------------------------------------------- accepted --
#
# Names a document mentions that are not API names. Each says why, because an
# entry without a reason is one nobody can re-check.
ACCEPTED = {
    # Event identifiers from hv2-api's ontology, not callable tools.
    'vm.state_changed': 'event id in ontology.rs, not a tool',
    'agent.completed': 'event id in ontology.rs, not a tool',
    # Config keys quoted as counter-examples in the deployment guide's
    # "documented before -> what the build reads" table.
    'vm.default_cpus': 'config key quoted as a counter-example',
    'vm.default_memory_mb': 'config key quoted as a counter-example',
    'vm.max_vms': 'config key quoted as a counter-example',
    'agent.rate_limit_rpm': 'config key quoted as a counter-example',
    # Tools that do not exist, named in the pages that say they do not exist.
    'vm.upload': 'named only where the docs say it is unimplemented',
    'vm.download': 'named only where the docs say it is unimplemented',
    # Source files, caught by the dotted-name shape.
    'vm.rs': 'a filename',
    'fleet.rs': 'a filename',
    'config.rs': 'a filename',
}

NAMESPACES = ('vm', 'snapshot', 'gpu', 'agent', 'context', 'image', 'guest', 'fleet')
SKIP_DIRS = {'node_modules', '.git', 'target', 'reference'}


def read(path):
    try:
        return open(path, encoding='utf-8').read()
    except Exception:
        return ''


def walk(root, suffix):
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for f in files:
            if f.endswith(suffix):
                yield os.path.join(base, f)


def rel(path):
    return path.replace('\\', '/').lstrip('./')


# ------------------------------------------------------------------ routes --
def real_routes():
    # Every crate, not just hv2-api: hm-cli's MCP server registers /mcp/tools
    # and /mcp/call, and scanning one crate reported those two documented
    # routes as missing when they are real.
    routes, prefixes = set(), {''}
    for path in walk('crates', '.rs'):
        text = read(path)
        routes |= {m.group(1) for m in re.finditer(r'\.route\(\s*"([^"]+)"', text)}
        prefixes |= {m.group(1) for m in re.finditer(r'\.nest\(\s*"([^"]+)"', text)}
    return {norm(p + r) for r in routes for p in prefixes}


def norm(path):
    out = []
    for seg in path.strip('/').split('/'):
        if seg.startswith('{') or seg.startswith(':') or re.fullmatch(r'[0-9a-fA-F-]{6,}|\d+', seg):
            out.append('{}')
        else:
            out.append(seg)
    return '/' + '/'.join(out)


def documented_routes():
    found = {}
    for path in walk('.', '.md'):
        for m in re.finditer(r'(GET|POST|PUT|PATCH|DELETE)\s*\|?\s*`?(/[A-Za-z0-9_/{}:.\-]*)`?', read(path)):
            route = m.group(2).rstrip('.,;`')
            if route.startswith(('/api', '/agentic', '/mcp')) or route == '/health':
                found.setdefault(norm(route), set()).add(rel(path))
    return found


# ------------------------------------------------------------------- tools --
def real_tools():
    names = set()
    for path in walk('crates', '.rs'):
        text = read(path)
        names |= {m.group(1) for m in re.finditer(r'name: *"([a-z_]+\.[a-z_]+)"', text)}
        if 'examples' not in path and 'tests' not in path:
            names |= {m.group(1) for m in re.finditer(r'"([a-z_]+\.[a-z_]+)" *(?:=>|\|)', text)}
    return names


def documented_tools():
    found = {}
    for path in walk('.', '.md'):
        for m in re.finditer(r'`([a-z_]+\.[a-z_]+)`', read(path)):
            if m.group(1).split('.')[0] in NAMESPACES:
                found.setdefault(m.group(1), set()).add(rel(path))
    return found


# ------------------------------------------------------------------ report --
def main():
    if not os.path.isdir('crates/hv2-api'):
        print('run from the repository root')
        return 2

    routes, tools = real_routes(), real_tools()
    bad_routes = {r: w for r, w in sorted(documented_routes().items()) if r not in routes}
    bad_tools = {t: w for t, w in sorted(documented_tools().items())
                 if t not in tools and t not in ACCEPTED}

    print('  routes: %d registered, %d documented, %d missing'
          % (len(routes), len(documented_routes()), len(bad_routes)))
    print('  tools:  %d registered, %d documented, %d missing'
          % (len(tools), len(documented_tools()), len(bad_tools)))

    for label, bad in (('route', bad_routes), ('tool', bad_tools)):
        for name, where in bad.items():
            print('  NEW %-6s %-34s %s' % (label, name, ', '.join(sorted(where))))

    if bad_routes or bad_tools:
        print()
        print('  Each is documented and not in the code. Correct the document,')
        print('  or add it to ACCEPTED with the reason it is not an API name.')
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
