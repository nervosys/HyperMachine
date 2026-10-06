#!/usr/bin/env python3
"""Deploy one operator-configured Certbot lineage to a running Linux TLS bundle.

Copies the certificate/key into an immutable private generation, publishes the
manifest atomically, signals the pinned control-plane process, and verifies the
new leaf through fresh certificate-validated TLS connections. Failed activation
restores the previous manifest and attempts verified rollback. Requires OpenSSL.
"""
import argparse
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


def deploy(args):
    require(os.name == "posix" and hasattr(os, "pidfd_open")
            and hasattr(signal, "pidfd_send_signal"), "deployment requires Linux pidfd support")
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
        matches = [entry for entry in document["certificates"] if sorted(entry["names"]) == domains]
        require(len(matches) == 1, "deployment must match exactly one existing hostname group")
        entry = matches[0]
        old_cert, _ = read_file(Path(entry["cert_path"]), 1024 * 1024, symlinks=True)
        previous = leaf_hash(old_cert)
        new_cert, _ = read_file(args.lineage / "fullchain.pem", 1024 * 1024, symlinks=True)
        new_key, _ = read_file(args.lineage / "privkey.pem", 64 * 1024, symlinks=True)
        expected = leaf_hash(new_cert)
        require(openssl("x509", "-pubkey", "-noout", data=new_cert)
                == openssl("pkey", "-pubout", data=new_key), "certificate and private key differ")
        openssl("x509", "-checkend", "0", "-noout", data=new_cert)
        for domain in domains:
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
        wait_for_leaf(args, domains, previous, previous_context)
        old_directory = Path(entry["cert_path"]).parent
        if (old_cert == new_cert and old_directory == Path(entry["key_path"]).parent
                and old_directory.parent == args.generations):
            wait_for_leaf(args, domains, expected, context)
            return {"success": True, "names": domains, "previous_leaf_sha256": previous,
                    "active_leaf_sha256": expected, "generation": str(old_directory),
                    "activation_verified": True, "unchanged": True}
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
                except Exception as rollback:
                    raise RuntimeError("deployment failed and rollback activation is unconfirmed: " + str(rollback)) from error
                raise RuntimeError("deployment failed; previous manifest and TLS leaf restored") from error
            raise
        return {"success": True, "names": domains, "previous_leaf_sha256": previous,
                "active_leaf_sha256": expected, "generation": str(generation),
                "activation_verified": True}
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
    args = parser.parse_args()
    try:
        print(json.dumps(deploy(args), indent=2))
    except Exception as error:
        parser.exit(1, "Certificate deployment failed: " + str(error) + "\n")


if __name__ == "__main__":
    main()
