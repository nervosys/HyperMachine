#!/usr/bin/env python3
"""Deploy one operator-configured Certbot lineage to a running Linux TLS bundle.

Copies the certificate/key into an immutable private generation, publishes the
manifest atomically, signals the pinned control-plane process, and verifies the
new leaf through fresh certificate-validated TLS connections. Failed activation
restores the previous manifest and attempts verified rollback. Requires OpenSSL.
"""
import argparse
import base64
import fcntl
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import signal
import socket
import ssl
import stat
import subprocess
import tempfile
import time


def require(condition, message):
    if not condition:
        raise ValueError(message)


def name(value):
    require(isinstance(value, str) and len(value) <= 253 and value == value.lower()
            and "." in value and all(re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", label)
                                    for label in value.split(".")), "invalid exact DNS name")
    try:
        ipaddress.ip_address(value)
    except ValueError:
        return value
    raise ValueError("IP addresses are not SNI certificate names")


def read_file(path, limit, symlinks=False):
    require(path.is_absolute(), "paths must be absolute")
    flags = os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC
    if not symlinks:
        flags |= os.O_NOFOLLOW
    with os.fdopen(os.open(path, flags), "rb") as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode), "input must be a regular file")
        data = stream.read(limit + 1)
    require(len(data) <= limit, "input exceeds size limit")
    return data, metadata


def no_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON field")
        result[key] = value
    return result


def manifest(data):
    document = json.loads(data, object_pairs_hook=no_duplicate_keys)
    require(isinstance(document, dict) and "certificates" in document
            and document.keys() <= {"default", "certificates"}, "invalid bundle fields")
    entries = document["certificates"]
    require(isinstance(entries, list) and len(entries) <= 128, "invalid certificate entry count")
    seen = set()
    for entry in entries:
        require(isinstance(entry, dict) and entry.keys() == {"names", "cert_path", "key_path"},
                "invalid certificate entry fields")
        require(isinstance(entry["names"], list) and entry["names"], "empty certificate names")
        for value in entry["names"]:
            domain = name(value)
            require(domain not in seen, "duplicate certificate hostname")
            seen.add(domain)
        for key in ("cert_path", "key_path"):
            require(isinstance(entry[key], str) and Path(entry[key]).is_absolute(), "invalid bundle path")
    require(len(seen) <= 1024, "too many certificate names")
    fallback = document.get("default")
    if fallback is not None:
        require(isinstance(fallback, dict) and fallback.keys() == {"cert_path", "key_path"}
                and all(isinstance(value, str) and Path(value).is_absolute()
                        for value in fallback.values()), "invalid default certificate")
    return document


def deployment_entry(document, domains, provision=False):
    entries = document["certificates"]
    matches = [entry for entry in entries if sorted(name(value) for value in entry["names"]) == domains]
    if len(matches) == 1:
        return matches[0]
    require(not matches and provision, "deployment must match exactly one existing hostname group")
    existing = {name(value) for entry in entries for value in entry["names"]}
    require(not existing.intersection(domains), "new hostname group overlaps an existing group")
    require(len(entries) < 128 and len(existing) + len(domains) <= 1024, "new group exceeds bundle limits")
    require(document.get("default") is not None, "new group requires an existing default certificate for rollback")
    entry = dict(document["default"], names=list(domains))
    entries.append(entry)
    return entry


def openssl(*args, data=None):
    # Error text may contain private input; expose only the failed operation.
    result = subprocess.run(["openssl", *args], input=data, capture_output=True, timeout=10)
    require(result.returncode == 0, "OpenSSL rejected certificate or key: " + args[0])
    return result.stdout


def leaf_hash(certificate):
    return hashlib.sha256(openssl("x509", "-outform", "DER", data=certificate)).hexdigest()


def atomic_write(path, data, mode):
    fd, temporary = tempfile.mkstemp(prefix=".tls-manifest-", dir=path.parent)
    temporary = Path(temporary)
    try:
        with os.fdopen(fd, "wb") as stream:
            os.fchmod(stream.fileno(), mode)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def peer_hash(address, port, domain, context, timeout):
    with socket.create_connection((address, port), timeout=timeout) as raw:
        with context.wrap_socket(raw, server_hostname=domain) as stream:
            return hashlib.sha256(stream.getpeercert(binary_form=True)).hexdigest()


def wait_for_leaf(args, domains, expected, context):
    deadline = time.monotonic() + args.timeout
    pending = set(domains)
    while pending:
        for domain in tuple(pending):
            remaining = deadline - time.monotonic()
            require(remaining > 0, "TLS activation was not confirmed before timeout")
            try:
                actual = peer_hash(args.connect_address, args.port, domain, context, min(2, remaining))
                if actual == expected:
                    pending.remove(domain)
            except (OSError, ssl.SSLError):
                pass
        if pending:
            time.sleep(min(0.05, max(0, deadline - time.monotonic())))


def journal_path(args):
    return args.manifest.with_name(args.manifest.name + ".deploy.pending")


def clear_journal(args):
    journal_path(args).unlink(missing_ok=True)
    directory = os.open(args.manifest.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def pending_manifest(args, domains, original, previous, proposed, expected):
    def record(data, leaf):
        return {"bytes_base64": base64.b64encode(data).decode(),
                "sha256": hashlib.sha256(data).hexdigest(), "leaf_sha256": leaf}
    raw = (json.dumps({"version": 1, "names": domains, "control_plane": str(args.control_plane),
                       "original": record(original, previous), "proposed": record(proposed, expected)}) + "\n").encode()
    require(len(raw) <= 3 * 1024 * 1024, "deployment journal exceeds size limit")
    atomic_write(journal_path(args), raw, 0o600)


def retirement_identity(args, leaf, existing_group):
    expected = getattr(args, "expected_retirement_leaf", None)
    if expected is None:
        return
    require(getattr(args, "retire_group", False), "receipt pin requires retirement mode")
    require(isinstance(expected, str) and re.fullmatch(r"[0-9a-f]{64}", expected),
            "invalid retirement receipt pin")
    require(not existing_group or leaf == expected,
            "named certificate differs from retirement receipt; operator reconciliation required")


def recover_manifest(args, domains, current, metadata, pidfd, context):
    try:
        raw, owner = read_file(journal_path(args), 3 * 1024 * 1024)
    except FileNotFoundError:
        return current, False
    require(owner.st_uid == os.geteuid() and owner.st_nlink == 1 and stat.S_IMODE(owner.st_mode) == 0o600,
            "unsafe deployment journal")
    pending = json.loads(raw, object_pairs_hook=no_duplicate_keys)
    require(isinstance(pending, dict) and pending.keys() == {"version", "names", "control_plane", "original", "proposed"}
            and type(pending["version"]) is int and pending["version"] == 1
            and pending["names"] == domains and pending["control_plane"] == str(args.control_plane),
            "deployment journal configuration differs; reconcile before changing it")
    candidates = []
    for key in ("original", "proposed"):
        value = pending[key]
        require(isinstance(value, dict) and value.keys() == {"bytes_base64", "sha256", "leaf_sha256"},
                "invalid deployment journal record")
        data = base64.b64decode(value["bytes_base64"], validate=True)
        require(len(data) <= 1024 * 1024 and hashlib.sha256(data).hexdigest() == value["sha256"],
                "deployment journal manifest checksum mismatch")
        document = manifest(data)
        entry = deployment_entry(document, domains, (getattr(args, "provision_new_group", False) or getattr(args, "retire_group", False)))
        cert, _ = read_file(Path(entry["cert_path"]), 1024 * 1024, symlinks=True)
        require(leaf_hash(cert) == value["leaf_sha256"], "journal certificate generation changed")
        if key == "original":
            existing = any(sorted(name(item) for item in group["names"]) == domains
                           for group in document["certificates"])
            retirement_identity(args, value["leaf_sha256"], existing)
        candidates.append((data, value["leaf_sha256"]))
    require(current in [data for data, _ in candidates],
            "manifest changed outside pending deployment; operator reconciliation required")
    deadline = time.monotonic() + args.timeout
    while True:
        require(time.monotonic() < deadline, "interrupted deployment runtime identity is unconfirmed")
        try:
            observed = []
            for domain in domains:
                remaining = deadline - time.monotonic()
                require(remaining > 0, "interrupted deployment runtime identity is unconfirmed")
                observed.append(peer_hash(args.connect_address, args.port, domain, context, min(1, remaining)))
            chosen = next(((data, leaf) for data, leaf in candidates if all(value == leaf for value in observed)), None)
            if chosen is not None:
                break
        except (OSError, ssl.SSLError):
            pass
        time.sleep(0.05)
    selected, leaf = chosen
    if current != selected:
        # Only the two checksummed manifests from this private journal may be
        # reconciled. A noncooperating operator update is never overwritten.
        latest, _ = read_file(args.manifest, 1024 * 1024)
        require(latest == current, "manifest changed during interrupted deployment recovery")
        atomic_write(args.manifest, selected, stat.S_IMODE(metadata.st_mode))
        signal.pidfd_send_signal(pidfd, signal.SIGHUP)
        wait_for_leaf(args, domains, leaf, context)
    return selected, True


def deploy(args):
    require(os.name == "posix" and hasattr(os, "pidfd_open")
            and hasattr(signal, "pidfd_send_signal"), "deployment requires Linux pidfd support")
    retiring = getattr(args, "retire_group", False)
    retirement_identity(args, None, False)
    require(not (retiring and getattr(args, "provision_new_group", False)), "provision and retirement modes are exclusive")
    domains = sorted(name(domain) for domain in args.domain)
    require(domains and len(domains) <= 128 and len(domains) == len(set(domains)),
            "duplicate, empty or excessive deployment names")
    require(args.pid > 1 and 1 <= args.port <= 65535 and 0.1 <= args.timeout <= 60,
            "invalid process, port or timeout")
    # Only an IP address is allowed here: DNS lookups cannot introduce an
    # unbounded resolution delay or change the operator's selected endpoint.
    ipaddress.ip_address(args.connect_address)
    for path in (args.manifest, args.lineage, args.generations, args.control_plane):
        require(path.is_absolute(), "deployment paths must be absolute")
    if args.from_certbot:
        require(os.environ.get("RENEWED_LINEAGE") == str(args.lineage), "unexpected Certbot lineage")
        renewed = set(os.environ.get("RENEWED_DOMAINS", "").split())
        require(set(domains) <= renewed, "Certbot renewal omits configured hostname")
    directory = args.generations.lstat()
    require(stat.S_ISDIR(directory.st_mode) and directory.st_uid == os.geteuid()
            and stat.S_IMODE(directory.st_mode) == 0o700, "generation directory must be owned and mode 0700")
    parent = args.manifest.parent.lstat()
    require(stat.S_ISDIR(parent.st_mode) and parent.st_uid == os.geteuid() and parent.st_mode & 0o022 == 0,
            "manifest directory must be owned and not writable by others")
    lock_path = args.manifest.with_name(args.manifest.name + ".deploy.lock")
    lock = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    pidfd = None
    try:
        lock_stat = os.fstat(lock)
        require(stat.S_ISREG(lock_stat.st_mode) and lock_stat.st_uid == os.geteuid()
                and lock_stat.st_nlink == 1 and stat.S_IMODE(lock_stat.st_mode) == 0o600,
                "unsafe deployment lock")
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        original, metadata = read_file(args.manifest, 1024 * 1024)
        require(metadata.st_uid == os.geteuid() and metadata.st_nlink == 1,
                "manifest must be owned with a single hard link")
        require(metadata.st_mode & 0o022 == 0, "manifest must not be writable by group or others")
        document = manifest(original)
        existing_group = any(sorted(name(value) for value in item["names"]) == domains
                             for item in document["certificates"])
        entry = deployment_entry(document, domains, (getattr(args, "provision_new_group", False) or getattr(args, "retire_group", False)))
        old_cert, _ = read_file(Path(entry["cert_path"]), 1024 * 1024, symlinks=True)
        previous = leaf_hash(old_cert)
        retirement_identity(args, previous, existing_group)
        if retiring:
            require(document.get("default") is not None, "retirement requires an existing default certificate")
            new_cert, _ = read_file(Path(document["default"]["cert_path"]), 1024 * 1024, symlinks=True)
            new_key, _ = read_file(Path(document["default"]["key_path"]), 64 * 1024, symlinks=True)
        else:
            new_cert, _ = read_file(args.lineage / "fullchain.pem", 1024 * 1024, symlinks=True)
            new_key, _ = read_file(args.lineage / "privkey.pem", 64 * 1024, symlinks=True)
        expected = leaf_hash(new_cert)
        require(openssl("x509", "-pubkey", "-noout", data=new_cert)
                == openssl("pkey", "-pubout", data=new_key), "certificate and private key differ")
        openssl("x509", "-checkend", "0", "-noout", data=new_cert)
        for domain in ([] if retiring else domains):
            # -checkhost prints a mismatch but exits zero on some OpenSSL versions.
            output = openssl("x509", "-checkhost", domain, "-noout", data=new_cert)
            require(b"does match certificate" in output, "certificate omits configured hostname")
        pidfd = os.pidfd_open(args.pid)
        executable = Path(f"/proc/{args.pid}/exe").stat()
        selected = args.control_plane.stat()
        require((executable.st_dev, executable.st_ino) == (selected.st_dev, selected.st_ino),
                "process executable differs from configured control plane")
        command, _ = read_file(Path(f"/proc/{args.pid}/cmdline"), 65536)
        argv = command.rstrip(b"\0").decode().split("\0")
        require(argv.count("--tls-bundle-file") == 1 and
                argv[argv.index("--tls-bundle-file") + 1] == str(args.manifest),
                "process uses a different TLS manifest")
        context = ssl.create_default_context(cafile=str(args.ca_file) if args.ca_file else None)
        # Pin the old leaf to the operator-owned manifest rather than requiring
        # its validity period/issuer to remain acceptable. Otherwise a missed
        # renewal could never recover an already-expired active certificate.
        # This connection sends no application data. New activation always uses
        # ordinary issuer, hostname and validity verification through `context`.
        previous_context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        previous_context.check_hostname = False
        previous_context.verify_mode = ssl.CERT_NONE
        if retiring:
            context = previous_context  # operator-pinned fallback; no application data
        original, recovered = recover_manifest(args, domains, original, metadata, pidfd, previous_context)
        if recovered:
            document = manifest(original)
            existing_group = any(sorted(name(value) for value in item["names"]) == domains
                                 for item in document["certificates"])
            entry = deployment_entry(document, domains, (getattr(args, "provision_new_group", False) or getattr(args, "retire_group", False)))
            old_cert, _ = read_file(Path(entry["cert_path"]), 1024 * 1024, symlinks=True)
            previous = leaf_hash(old_cert)
            retirement_identity(args, previous, existing_group)
        wait_for_leaf(args, domains, previous, previous_context)
        old_directory = Path(entry["cert_path"]).parent
        if ((retiring and not existing_group) or (not retiring and existing_group and old_cert == new_cert and old_directory == Path(entry["key_path"]).parent
                and old_directory.parent == args.generations)):
            wait_for_leaf(args, domains, expected, context)
            clear_journal(args)
            return {"success": True, "names": domains, "previous_leaf_sha256": previous,
                    "active_leaf_sha256": expected, "generation": str(old_directory),
                    "activation_verified": True, "unchanged": True, "retired": retiring, "recovered_pending_deployment": recovered}
        if retiring:
            generation = old_directory
            document["certificates"].remove(entry)
        else:
            generation = Path(tempfile.mkdtemp(prefix="certificate-", dir=args.generations))
            for filename, content in (("fullchain.pem", new_cert), ("privkey.pem", new_key)):
                fd = os.open(generation / filename, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(fd, "wb") as stream:
                    stream.write(content)
                    stream.flush()
                    os.fsync(stream.fileno())
            for path in (generation, args.generations):
                fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
                try:
                    os.fsync(fd)
                finally:
                    os.close(fd)
            entry.update(cert_path=str(generation / "fullchain.pem"), key_path=str(generation / "privkey.pem"))
        proposed = (json.dumps(document, indent=2) + "\n").encode()
        require(len(proposed) <= 1024 * 1024, "proposed manifest exceeds size limit")
        published = False
        try:
            # Check for concurrent writers which do not honor the deployment lock.
            current, _ = read_file(args.manifest, 1024 * 1024)
            require(current == original, "manifest changed during deployment")
            pending_manifest(args, domains, original, previous, proposed, expected)
            published = True  # includes a replace followed by a failed directory fsync
            atomic_write(args.manifest, proposed, stat.S_IMODE(metadata.st_mode))
            signal.pidfd_send_signal(pidfd, signal.SIGHUP)
            wait_for_leaf(args, domains, expected, context)
        except Exception as error:
            if published:
                try:
                    # Never discard another writer's update during rollback.
                    current, _ = read_file(args.manifest, 1024 * 1024)
                    require(current in (original, proposed), "manifest changed; rollback requires operator reconciliation")
                    atomic_write(args.manifest, original, stat.S_IMODE(metadata.st_mode))
                    signal.pidfd_send_signal(pidfd, signal.SIGHUP)
                    wait_for_leaf(args, domains, previous, previous_context)
                    clear_journal(args)
                except Exception as rollback:
                    raise RuntimeError("deployment failed and rollback activation is unconfirmed: " + str(rollback)) from error
                raise RuntimeError("deployment failed; previous manifest and TLS leaf restored") from error
            raise
        clear_journal(args)
        return {"success": True, "names": domains, "previous_leaf_sha256": previous,
                "active_leaf_sha256": expected, "generation": str(generation),
                "activation_verified": True, "retired": retiring, "recovered_pending_deployment": recovered}
    finally:
        if pidfd is not None:
            os.close(pidfd)
        os.close(lock)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("manifest", "lineage", "generations", "control-plane"):
        parser.add_argument("--" + option, type=Path, required=True)
    parser.add_argument("--domain", action="append", required=True)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--port", type=int, default=443)
    parser.add_argument("--connect-address", default="127.0.0.1")
    parser.add_argument("--ca-file", type=Path)
    parser.add_argument("--timeout", type=float, default=10)
    parser.add_argument("--from-certbot", action="store_true")
    parser.add_argument("--provision-new-group", action="store_true",
                        help="Allow a non-overlapping new hostname group; requires an existing default certificate")
    parser.add_argument("--retire-group", action="store_true", help="Remove an exact named group and verify the pinned default; retain immutable files")
    parser.add_argument("--expected-retirement-leaf", help="Require an existing named certificate to match this verified SHA-256 receipt")
    args = parser.parse_args()
    try:
        print(json.dumps(deploy(args), indent=2))
    except Exception as error:
        parser.exit(1, "Certificate deployment failed: " + str(error) + "\n")


if __name__ == "__main__":
    main()
