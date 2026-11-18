#!/usr/bin/env python3
"""Per-role Redis ACL for a HyperMachine cluster (security finding O12).

Every node (hv2-sandboxd --cluster-store), every control plane (hv2-control-plane) and the
native gateway share one Redis. On one shared Redis user a node can DEL a managed API key's
sealed revocation record and write the key's old record back, which revives a revoked key.
Code cannot stop a Redis writer from deleting, so this prints one Redis 7+ ACL user per role
and takes the key and tombstone keyspace away from the nodes.

    HV2_ACL_CONTROL_PLANE_PASSWORD=... HV2_ACL_NODE_PASSWORD=... HV2_ACL_GATEWAY_PASSWORD=... \\
        tools/redis-acl.py --namespace default > users.acl        # an aclfile
    tools/redis-acl.py --namespace default --format commands      # ACL SETUSER lines for redis-cli

Passwords come from the environment and are written as SHA-256 hashes (`#<hex>`), so the
output holds no secret. Use long random passwords (hex), which need no escaping in a
redis:// URL. The `default` user is turned off.

THE TABLE below is the single source of truth: the key families of crates/hv2-cluster/src/
store.rs (RedisStore::key, prefix `hv2:<namespace>:`), and which role may read (r) or write
(w) each. If store.rs gains a key family, add a row here; `--check-source FILE` fails if
store.rs builds a family that this table does not name.
"""
import argparse, hashlib, os, re, sys

# family -> access per role. A trailing `*` in a family is a glob, matching `family:<id>`.
# The key/tombstone families are control-plane only. The node never validates a managed key
# (only the control plane's authentication does) and never serves the team-telemetry routes.
#                   family                    control-plane  node   gateway
FAMILIES = [
    ("nodes",                  "rw", "rw", "r"),
    ("node:*",                 "rw", "rw", "r"),
    ("sandboxes",              "rw", "rw", "r"),
    ("sandbox:*",              "rw", "rw", "r"),
    ("public-ports",           "rw", "rw", "r"),
    ("ports:*",                "rw", "rw", "r"),
    ("private-membership:*",   "rw", "rw", ""),
    ("web-sharing:*",          "rw", "rw", ""),
    ("private-addresses:*",   "rw", "rw", ""),
    ("name-reservations",      "rw", "rw", ""),
    ("reserved-names:*",       "rw", "rw", ""),
    ("domains",                "rw", "rw", ""),
    ("domains:*",              "rw", "rw", ""),
    ("events",                 "rw", "rw", ""),
    ("usage:since",            "rw", "rw", ""),
    ("usage:open",             "rw", "rw", ""),
    ("usage:closed",           "rw", "rw", ""),
    ("environments",           "rw", "rw", ""),
    ("webhooks",               "rw", "rw", ""),
    ("deliveries:*",           "rw", "rw", ""),
    ("team-telemetry",         "rw", "",   ""),
    ("managed-keys",           "rw", "",   ""),
    ("managed-key-ids",        "rw", "",   ""),
    ("managed-key-tombstones", "rw", "",   ""),
]
ROLES = [("control-plane", "hv2-control-plane", 1), ("node", "hv2-node", 2), ("gateway", "hv2-gateway", 3)]

# Commands: the store uses reads, writes (including XADD), MULTI/EXEC (node registration and
# removal are atomic pipelines; with MULTI refused, redis-rs 1.7.1 panics in its connection task
# instead of returning an error), EVALSHA/EVAL for its Lua scripts (SCRIPT LOAD on a cache miss)
# and the connection setup. No pub/sub, no KEYS/SCAN, no admin.
# Redis checks the caller's key patterns again for every key a script touches.
COMMANDS = {
    "rw": ["+@read", "+@write", "+@transaction", "+@connection", "+eval", "+evalsha", "+script|load", "-@dangerous"],
    # The gateway only reads. EVAL needs no write permission for a script that does not write;
    # the key patterns (%R~) make a write inside a script fail too.
    "r": ["+@read", "+@connection", "+eval_ro", "+evalsha_ro", "+eval", "+evalsha", "+script|load", "-@dangerous"],
}


def keyspec(namespace, role_index):
    patterns = []
    for row in FAMILIES:
        access = row[role_index]
        if access:
            patterns.append(("~" if access == "rw" else "%R~") + f"hv2:{namespace}:{row[0]}")
    return patterns


def user_rules(namespace, role_index, password):
    access = {row[role_index] for row in FAMILIES if row[role_index]}
    commands = COMMANDS["rw" if "rw" in access else "r"]
    digest = hashlib.sha256(password.encode()).hexdigest()
    return ["on", "#" + digest, "resetkeys", "resetchannels", "-@all"] + keyspec(namespace, role_index) + commands


def source_families(path):
    text = open(path, encoding="utf-8").read()
    found = set()
    for match in re.finditer(r'self\.key\(\s*(?:&format!\(\s*)?"([a-z0-9:-]*)', text):
        name = match.group(1)
        found.add(name + "*" if name.endswith(":") else name)
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--namespace", default="default", help="the --namespace / --cluster-namespace the cluster uses")
    parser.add_argument("--format", choices=["aclfile", "commands"], default="aclfile")
    parser.add_argument("--check-source", metavar="STORE_RS", help="fail if store.rs builds a key family not in the table")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9._-]+", args.namespace):
        sys.exit("namespace must be [A-Za-z0-9._-]+ (a glob character would widen the ACL)")
    if args.check_source:
        found, table = source_families(args.check_source), {row[0] for row in FAMILIES}
        if found != table:
            sys.exit(f"store.rs and the table differ: only in store.rs {sorted(found - table)}, only in table {sorted(table - found)}")
        print(f"ok: {len(found)} key families match")
        return
    lines = []
    for role, user, index in ROLES:
        variable = "HV2_ACL_" + role.upper().replace("-", "_") + "_PASSWORD"
        password = os.environ.get(variable, "")
        if len(password) < 24:
            sys.exit(f"{variable} must be set to a password of at least 24 characters")
        rules = user_rules(args.namespace, index, password)
        lines.append(("user " + user + " " if args.format == "aclfile" else "ACL SETUSER " + user + " ") + " ".join(rules))
    lines.append("user default off" if args.format == "aclfile" else "ACL SETUSER default off")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
