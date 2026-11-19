#!/usr/bin/env python3
"""Every HTTP route is either described in the agent ontology or allowlisted.

The agent ontology (crates/hv2-api/src/ontology.rs, served at
/agentic/ontology) is meant to describe every interface. This gate keeps
that honest while the gaps are closed one area at a time:

  * It extracts every route (METHOD + path) from the axum route tables of
    hv2-api, hv2-cluster, hv2-sandboxd, hm-cli and hv2-jobs.
  * It reads the ontology's declared operations from the checked-in
    docs/ontology-operations.json, which a hv2-api test keeps identical to
    `HyperMachineOntology::build()` (the test fails when it is stale, and
    UPDATE_ONTOLOGY_OPERATIONS=1 rewrites it). No cargo is needed here.
  * tools/ontology-coverage-allowlist.txt lists the routes known to be
    undescribed today.

It fails when
  (a) a route exists in code that is neither declared nor allowlisted,
  (b) an allowlisted route is now declared, or no longer exists in code.
So the allowlist can only shrink. A declared operation that no route serves
is reported as a warning (the ontology promises an interface that is gone,
or lives in a component this gate does not scan).

Extraction. Routes are read from source text, not from a running router:
comments are blanked and string interiors masked (so a `.route(` inside a
string or a doc comment is not seen), `#[cfg(test)]` items, `#[test]`
functions and test-only files are dropped, then every `.route(PATH, METHODS)`
call is parsed with balanced parentheses, so multi-line routes and
`get(a).post(b).layer(..)` chains are read the same as one-liners.
`.nest("/prefix", router_fn(..))` is resolved to the function that builds the
nested router and its routes get the prefix. Anything this parser cannot read
with certainty (a non-literal path, a `.route` with no recognisable method, a
`.nest` it cannot resolve) is a hard error, never a silent skip. Path
parameter names are ignored when comparing ({id} == {sandboxID}). `.merge(..)`
needs no handling: a merged router's routes are in its own function.

Usage:
  python3 tools/check-ontology-coverage.py            # the gate
  python3 tools/check-ontology-coverage.py --list     # every route found
  python3 tools/check-ontology-coverage.py --write-allowlist   # regenerate
  python3 tools/check-ontology-coverage.py --self-test
"""
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MANIFEST = os.path.join(ROOT, "docs", "ontology-operations.json")
ALLOWLIST = os.path.join(ROOT, "tools", "ontology-coverage-allowlist.txt")

# component -> source directory (relative to the repository root)
COMPONENTS = {
    "hv2-api": "crates/hv2-api/src",
    "hv2-cluster": "crates/hv2-cluster/src",
    "hv2-sandboxd": "crates/hv2-sandboxd/src",
    "hm-cli": "crates/hm-cli/src",
    "hv2-jobs": "crates/hv2-jobs/src",
}

METHODS = ["get", "post", "put", "delete", "patch", "head", "options", "trace", "any"]
METHOD_RE = re.compile(r"(?:%s)(?:_service)?\b" % "|".join(METHODS))
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


class ExtractError(Exception):
    pass


# ---------------------------------------------------------------------------
# Masking: same length as the source, so offsets map back to the original.
# ---------------------------------------------------------------------------
def mask(src):
    """Blank comments and the interior of string/char literals.

    Newlines are kept so line numbers still work. Quotes are kept, so a
    masked `"xxxx"` is still recognisable as a string whose text lives at the
    same offsets in `src`.
    """
    out = list(src)
    n = len(src)
    i = 0

    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif c == "/" and nxt == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif c == "r" and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")) and re.match(r'r#*"', src[i:i + 40]):
            m = re.match(r'r(#*)"', src[i:i + 40])
            hashes = m.group(1)
            start = i + m.end()
            end = src.find('"' + hashes, start)
            if end < 0:
                raise ExtractError("unterminated raw string")
            blank(start, end)
            i = end + 1 + len(hashes)
        elif c == "b" and nxt == '"' and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            i += 1  # the quote is handled on the next turn
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(i + 1, j)
            i = j + 1
        elif c == "'":
            # a char literal ('a', '\n', '\u{..}') or a lifetime ('a)
            m = re.match(r"'(\\.[^']*|[^\\'])'", src[i:i + 12])
            if m:
                blank(i + 1, i + m.end() - 1)
                i += m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def match_close(m, open_idx):
    """Index of the bracket closing the one at open_idx, in masked text."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = []
    for k in range(open_idx, len(m)):
        ch = m[k]
        if ch in pairs:
            stack.append(pairs[ch])
        elif ch in ")]}":
            if not stack or stack.pop() != ch:
                raise ExtractError("unbalanced brackets")
            if not stack:
                return k
    raise ExtractError("unclosed bracket")


def split_top(m, a, b):
    """Split m[a:b] at top-level commas; returns [(start, end)]."""
    parts, depth, s = [], 0, a
    for k in range(a, b):
        ch = m[k]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif ch == "," and depth == 0:
            parts.append((s, k))
            s = k + 1
    if m[s:b].strip():
        parts.append((s, b))
    return parts


# ---------------------------------------------------------------------------
# Test code removal.
# ---------------------------------------------------------------------------
CFG_TEST = re.compile(r"#\[\s*cfg\s*\(\s*(?:test|all\s*\(\s*test\b[^\]]*?)\)\s*\]|#\[\s*(?:tokio::)?test\b[^\]]*\]")


def drop_test_items(src, m):
    """Blank `#[cfg(test)]` items and `#[test]` fns in m (and return the
    names of `#[cfg(test)] mod name;` files)."""
    out = list(m)
    test_mods = []
    for hit in CFG_TEST.finditer(m):
        if out[hit.start()] == " ":
            continue  # inside an already removed item
        j = hit.end()
        # further attributes
        while True:
            mm = re.match(r"\s*#\[", m[j:])
            if not mm:
                break
            j = match_close(m, j + mm.end() - 1) + 1
        # the item: up to its `{...}` body or its `;`
        depth, k = 0, j
        end = None
        is_item = re.match(
            r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:unsafe|async|const|extern(?:\s+\"[^\"]*\")?)\s+)*"
            r"(?:fn|mod|impl|struct|enum|trait|use|static|type|const)",
            m[j:j + 120],
        )
        while k < len(m):
            ch = m[k]
            if ch in "([":
                depth += 1
            elif ch in ")]}":
                depth -= 1
                if depth < 0:  # the enclosing struct/block ends: a field
                    end = k
                    break
            elif ch == ";" and depth == 0 or ch == "," and depth == 0 and not is_item:
                end = k + 1
                break
            elif ch == "{" and depth == 0:
                end = match_close(m, k) + 1
                break
            k += 1
        if end is None:
            raise ExtractError("cannot find the end of a #[cfg(test)] item")
        item = m[j:end]
        mod = re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;", item)
        if mod:
            test_mods.append(mod.group(1))
        for p in range(hit.start(), end):
            if out[p] != "\n":
                out[p] = " "
    return "".join(out), test_mods


# ---------------------------------------------------------------------------
# Route parsing.
# ---------------------------------------------------------------------------
def line_of(src, idx):
    return src.count("\n", 0, idx) + 1


def fn_spans(m):
    """[(name, body_start, body_end)] for every fn with a body."""
    spans = []
    for hit in re.finditer(r"\bfn\s+(\w+)", m):
        depth, k = 0, hit.end()
        while k < len(m):
            ch = m[k]
            if ch in "([":
                depth += 1
            elif ch in ")]":
                depth -= 1
            elif ch == ";" and depth == 0:
                break
            elif ch == "{" and depth == 0:
                spans.append((hit.group(1), k, match_close(m, k)))
                break
            k += 1
    return spans


def enclosing_fn(spans, pos):
    best = None
    for name, a, b in spans:
        if a <= pos <= b and (best is None or b - a < best[2] - best[1]):
            best = (name, a, b)
    return best[0] if best else None


def methods_of(m, a, b, where):
    found, depth, k = [], 0, a
    while k < b:
        ch = m[k]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif depth == 0 and (ch.isalpha() or ch == "_") and (k == a or not (m[k - 1].isalnum() or m[k - 1] == "_")):
            mm = IDENT.match(m, k)
            word = mm.group(0)
            after = m[mm.end():b].lstrip()
            if after.startswith("("):
                base = word[:-8] if word.endswith("_service") else word
                if base in METHODS:
                    found.append("ANY" if base == "any" else base.upper())
                elif word == "fallback":
                    # `put(h).fallback(f)`: f answers the methods the route
                    # does not serve (405); it adds none to the route.
                    pass
                elif word in ("on", "on_service", "route_service", "nest_service"):
                    raise ExtractError("%s: `%s(` in a route is not understood" % (where, word))
            k = mm.end()
            continue
        k += 1
    if not found:
        raise ExtractError("%s: no HTTP method found in route arguments" % where)
    return found


def extract_file(src, relname):
    """-> (routes, nests): routes = [(fn, path, METHOD, line)],
    nests = [(prefix, ident_or_fn, line, enclosing_fn)]"""
    m = mask(src)
    if re.search(r"#!\[\s*cfg\s*\(\s*test\s*\)\s*\]", m):
        return [], [], []
    m, test_mods = drop_test_items(src, m)
    spans = fn_spans(m)
    routes, nests = [], []
    for hit in re.finditer(r"\.\s*route\s*\(", m):
        open_idx = hit.end() - 1
        close = match_close(m, open_idx)
        where = "%s:%d" % (relname, line_of(src, hit.start()))
        args = split_top(m, open_idx + 1, close)
        if len(args) != 2:
            raise ExtractError("%s: .route( does not have two arguments" % where)
        a1 = src[args[0][0]:args[0][1]].strip()
        lit = re.fullmatch(r'"([^"\\]*)"', a1)
        if not lit:
            raise ExtractError("%s: route path is not a plain string literal: %s" % (where, a1[:60]))
        path = lit.group(1)
        if not path.startswith("/"):
            raise ExtractError("%s: route path %r does not start with /" % (where, path))
        fn = enclosing_fn(spans, hit.start())
        for method in methods_of(m, args[1][0], args[1][1], where):
            routes.append((fn, path, method, line_of(src, hit.start())))
    for hit in re.finditer(r"\.\s*nest\s*\(", m):
        open_idx = hit.end() - 1
        close = match_close(m, open_idx)
        where = "%s:%d" % (relname, line_of(src, hit.start()))
        args = split_top(m, open_idx + 1, close)
        if len(args) != 2:
            raise ExtractError("%s: .nest( does not have two arguments" % where)
        a1 = src[args[0][0]:args[0][1]].strip()
        lit = re.fullmatch(r'"([^"\\]*)"', a1)
        if not lit:
            raise ExtractError("%s: nest prefix is not a plain string literal" % where)
        nests.append((lit.group(1), m[args[1][0]:args[1][1]].strip(), where, m))
    return routes, nests, test_mods


def resolve_nest_target(expr, file_mask):
    """The function name that builds a nested router: `f(..)`/`a::f(..)`
    directly, or a local `let ident = a::f(..)` for a bare identifier."""
    if re.fullmatch(r"[A-Za-z_]\w*", expr):
        mm = re.search(r"\blet\s+(?:mut\s+)?%s\s*(?::[^=]+)?=\s*([^;]+);" % re.escape(expr), file_mask)
        if not mm:
            return None
        expr = mm.group(1).strip()
    call = re.match(r"(?:[A-Za-z_]\w*::)*([A-Za-z_]\w*)\s*(?:::<[^>]*>)?\s*\(", expr)
    return call.group(1) if call else None


def source_files(directory):
    out = []
    for dirpath, dirnames, filenames in os.walk(directory):
        dirnames.sort()
        for name in sorted(filenames):
            if name.endswith(".rs"):
                out.append(os.path.join(dirpath, name))
    return out


def read(path):
    with open(path, "rb") as f:
        return f.read().decode("utf-8").replace("\r\n", "\n")


def norm_path(path):
    path = re.sub(r"\{[^}]*\}", "{}", path)
    if len(path) > 1:
        path = path.rstrip("/")
    return path


def extract_component(component, rel_dir):
    """-> {(METHOD, normalized path): (display path, 'file:line')}"""
    base = os.path.join(ROOT, *rel_dir.split("/"))
    files = source_files(base)
    parsed = {}
    test_only = set()
    for path in files:
        rel = os.path.relpath(path, ROOT).replace(os.sep, "/")
        name = os.path.basename(path)
        if name.endswith("_tests.rs") or name in ("tests.rs",) or "/tests/" in rel:
            test_only.add(path)
            continue
        src = read(path)
        try:
            routes, nests, test_mods = extract_file(src, rel)
        except ExtractError as e:
            raise ExtractError("%s: %s" % (rel, e))
        parsed[path] = (src, routes, nests)
        for mod in test_mods:
            test_only.add(os.path.join(os.path.dirname(path), mod + ".rs"))
            test_only.add(os.path.join(os.path.dirname(path), mod, "mod.rs"))
    for path in test_only:
        parsed.pop(path, None)

    # fn name -> files defining a routed fn of that name
    fn_files = {}
    for path, (_, routes, _) in parsed.items():
        for fn, _, _, _ in routes:
            fn_files.setdefault(fn, set()).add(path)

    prefixes = {}  # (path, fn) -> prefix, for nested fns
    for path, (src, routes, nests) in parsed.items():
        rel = os.path.relpath(path, ROOT).replace(os.sep, "/")
        for prefix, expr, where, fmask in nests:
            target = resolve_nest_target(expr, fmask)
            owners = fn_files.get(target, set()) if target else set()
            if not target or len(owners) != 1:
                raise ExtractError(
                    "%s: cannot resolve .nest(%r, %s) to one router function (found %s)"
                    % (where, prefix, expr[:50], sorted(os.path.basename(o) for o in owners))
                )
            key = (next(iter(owners)), target)
            if key in prefixes and prefixes[key] != prefix:
                raise ExtractError("%s: router %s nested under two prefixes" % (where, target))
            prefixes[key] = prefix

    result = {}
    for path, (src, routes, _) in parsed.items():
        rel = os.path.relpath(path, ROOT).replace(os.sep, "/")
        for fn, rpath, method, line in routes:
            full = prefixes.get((path, fn), "") + rpath
            key = (method, norm_path(full))
            where = "%s:%d" % (rel, line)
            if key not in result or where < result[key][1]:
                result[key] = (full, where)
    return result


def extract_all():
    return {c: extract_component(c, d) for c, d in COMPONENTS.items()}


# ---------------------------------------------------------------------------
# Declared operations and the allowlist.
# ---------------------------------------------------------------------------
def load_declared():
    with open(MANIFEST, "rb") as f:
        doc = json.loads(f.read().decode("utf-8"))
    declared = {}
    for op in doc["operations"]:
        declared[(op["component"], op["method"].upper(), norm_path(op["path"]))] = op["id"]
    return declared


def load_allowlist():
    allowed = {}
    with open(ALLOWLIST, "rb") as f:
        text = f.read().decode("utf-8").replace("\r\n", "\n")
    for n, line in enumerate(text.split("\n"), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) != 3 or parts[0] not in COMPONENTS:
            raise SystemExit("%s:%d: expected `component METHOD /path`: %r" % (ALLOWLIST, n, line))
        key = (parts[0], parts[1], norm_path(parts[2]))
        if key in allowed:
            raise SystemExit("%s:%d: duplicate entry %r" % (ALLOWLIST, n, line))
        allowed[key] = n
    return allowed


def write_allowlist(routes, declared):
    lines = [
        "# Routes the agent ontology does not describe yet. `component METHOD /path`.",
        "# tools/check-ontology-coverage.py fails if a route in code is in neither",
        "# this file nor docs/ontology-operations.json, and fails if an entry here",
        "# is now declared or no longer exists: the list can only shrink. To remove",
        "# an entry, describe the route in crates/hv2-api/src/ontology.rs.",
        "# Regenerate with `--write-allowlist` only when adding a component.",
        "",
    ]
    for component in COMPONENTS:
        groups = {}
        for (method, np), (full, where) in routes[component].items():
            if (component, method, np) in declared:
                continue
            groups.setdefault(where.rsplit(":", 1)[0], []).append((full, method))
        for file in sorted(groups):
            lines.append("# %s: %s" % (component, file))
            for full, method in sorted(groups[file]):
                lines.append("%s %s %s" % (component, method, full))
            lines.append("")
    with open(ALLOWLIST, "wb") as f:
        f.write("\n".join(lines).encode("utf-8"))


def main(argv):
    if "--self-test" in argv:
        return self_test()
    routes = extract_all()
    if "--list" in argv:
        for component in COMPONENTS:
            for (method, np), (full, where) in sorted(routes[component].items(), key=lambda kv: (kv[0][1], kv[0][0])):
                print("%s %s %s  (%s)" % (component, method, full, where))
        return 0
    declared = load_declared()
    if "--write-allowlist" in argv:
        write_allowlist(routes, declared)
        return 0
    allowed = load_allowlist()

    failures, warnings = [], []
    code_keys = set()
    print("%-14s %7s %9s %11s %9s" % ("component", "routes", "declared", "allowlisted", "missing"))
    for component in COMPONENTS:
        n_decl = n_allow = n_missing = 0
        for (method, np), (full, where) in sorted(routes[component].items(), key=lambda kv: (kv[0][1], kv[0][0])):
            key = (component, method, np)
            code_keys.add(key)
            if key in declared:
                n_decl += 1
                if key in allowed:
                    failures.append(
                        "allowlisted but now declared (remove it from the allowlist, line %d): %s %s %s"
                        % (allowed[key], component, method, full)
                    )
            elif key in allowed:
                n_allow += 1
            else:
                n_missing += 1
                failures.append(
                    "not in the ontology and not allowlisted: %s %s %s  (%s)" % (component, method, full, where)
                )
        print("%-14s %7d %9d %11d %9d" % (component, len(routes[component]), n_decl, n_allow, n_missing))
    for key, line in sorted(allowed.items(), key=lambda kv: kv[1]):
        if key not in code_keys:
            failures.append(
                "allowlisted but no such route in code (line %d): %s %s %s" % (line, key[0], key[1], key[2])
            )
    for key, op_id in sorted(declared.items()):
        if key not in code_keys:
            warnings.append("declared but no route found in code: %s %s %s (%s)" % (key[0], key[1], key[2], op_id))
    for w in warnings:
        print("warning:", w)
    if failures:
        print()
        for f in failures:
            print("FAIL:", f)
        print("\n%d problem(s). Describe the route in the ontology (crates/hv2-api/src/ontology.rs,"
              " then UPDATE_ONTOLOGY_OPERATIONS=1 cargo test -p hv2-api --test ontology_tests),"
              " or, for a pre-existing gap only, list it in tools/ontology-coverage-allowlist.txt." % len(failures))
        return 1
    print("\nok: %d allowlisted route(s) remain undescribed" % len(allowed))
    return 0


# ---------------------------------------------------------------------------
# Self-test: the extractor reads what it should, including the hard cases.
# ---------------------------------------------------------------------------
def self_test():
    sample = r'''
//! a doc comment with .route("/in/doc", get(x))
use axum::Router;
pub fn router() -> Router {
    let note = ".route(\"/in/string\", get(x))";
    Router::new()
        .route("/one", get(a))
        .route(
            "/multi/{id}",
            get(h)
                .post(h2)
                .layer(DefaultBodyLimit::max(10)),
        )
        .route("/w/{id}", axum::routing::delete(d).put(|| async { get(nope) }))
        // .route("/commented", get(c))
        .route("/any", any(f))
        .route_layer(mw)
}
pub fn nested() -> Router { Router::new().route("/inner", get(i)) }
pub fn build() { let r = nested(); let app = Router::new().nest("/pre", r); }
#[cfg(test)]
mod tests { fn t() { Router::new().route("/test-only", get(x)); } }
#[tokio::test]
async fn lone() { Router::new().route("/lone-test", get(x)); }
'''
    routes, nests, _ = extract_file(sample, "sample.rs")
    got = sorted((p, m) for _, p, m, _ in routes)
    want = sorted([
        ("/one", "GET"), ("/multi/{id}", "GET"), ("/multi/{id}", "POST"),
        ("/w/{id}", "DELETE"), ("/w/{id}", "PUT"), ("/any", "ANY"), ("/inner", "GET"),
    ])
    assert got == want, (got, want)
    assert len(nests) == 1 and nests[0][0] == "/pre"
    assert resolve_nest_target("r", nests[0][3]) == "nested"
    assert norm_path("/a/{id}/b/{name}/") == "/a/{}/b/{}"
    for bad in ['Router::new().route(P, get(x))', 'Router::new().route("/x", handler)']:
        try:
            extract_file(bad, "bad.rs")
        except ExtractError:
            pass
        else:
            raise AssertionError("should refuse: " + bad)
    print("self-test ok")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except ExtractError as e:
        print("error:", e)
        sys.exit(2)
