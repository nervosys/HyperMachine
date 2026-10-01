#!/usr/bin/env python3
"""Build an isolated SSH test initrd from a TCP fixture and extracted Debian packages.

Generates disposable client and guest keys in output; never use them in production.
Requires Linux, cpio, ssh-keygen, openssl, ldd and extracted dropbear-bin dependencies.
"""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import tempfile


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--package-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    package = args.package_root.resolve()
    environment = dict(os.environ, LD_LIBRARY_PATH=str(package / "usr/lib/x86_64-linux-gnu"))
    server, keygen = (package / path for path in ["usr/sbin/dropbear", "usr/bin/dropbearkey"])
    report = {"success": False, "builder_sha256": digest(__file__), "base_sha256": digest(args.base),
              "fixture_only": True, "input_sha256": {}, "server_version": subprocess.check_output(
                  [server, "-V"], env=environment, stderr=subprocess.STDOUT, text=True).strip()}
    try:
        client = args.output / "client-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", client], check=True)
        with tempfile.TemporaryDirectory(prefix="hm-ssh-image-") as temporary:
            root = Path(temporary)
            archive = gzip.decompress(args.base.read_bytes())
            entries = subprocess.check_output(["cpio", "-t", "--quiet"], input=archive).decode().splitlines()
            assert all(not Path(name).is_absolute() and ".." not in Path(name).parts for name in entries)
            subprocess.run(["cpio", "-id", "--quiet", "--no-absolute-filenames"], input=archive, cwd=root, check=True)
            dependencies = set()
            for binary in [server, keygen]:
                output = subprocess.check_output(["ldd", binary], env=environment, text=True)
                if "not found" in output:
                    raise RuntimeError(output)
                dependencies.update(Path(match) for match in re.findall(r"(?:=>\s+|^\s*)(/\S+)", output, re.M))
            for source in sorted(dependencies | {server, keygen}):
                relative = source.relative_to(package) if source.is_relative_to(package) else Path(str(source).lstrip("/"))
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
                target.chmod(0o755)
                report["input_sha256"][str(source)] = digest(source)
            for directory in ["etc/dropbear", "root/.ssh", "var/run"]:
                (root / directory).mkdir(parents=True, exist_ok=True)
            (root / "root").chmod(0o700)
            (root / "root/.ssh").chmod(0o700)
            authorized = root / "root/.ssh/authorized_keys"
            authorized.write_bytes(client.with_suffix(".pub").read_bytes())
            authorized.chmod(0o600)
            (root / "etc/passwd").write_text("root:x:0:0:SSH fixture:/root:/bin/sh\n")
            password_hash = subprocess.check_output(["openssl", "passwd", "-6", "-stdin"],
                input=secrets.token_hex(32).encode()).decode().strip()
            (root / "etc/shadow").write_text(f"root:{password_hash}:20000:0:99999:7:::\n")
            (root / "etc/shadow").chmod(0o600)
            (root / "etc/group").write_text("root:x:0:\n")
            (root / "etc/shells").write_text("/bin/sh\n")
            host_key = root / "etc/dropbear/fixture-key"
            subprocess.run([keygen, "-t", "ed25519", "-f", host_key], env=environment,
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            public = subprocess.check_output([keygen, "-y", "-f", host_key], env=environment, text=True)
            public = next(line for line in public.splitlines() if line.startswith("ssh-ed25519 "))
            (root / "etc/dropbear/fixture-key.pub").write_text(public + "\n")
            (args.output / "host-key.pub").write_text(public + "\n")
            # Keep the base init/guest agent untouched; the E2E harness starts
            # this loopback-only, key-only server through authenticated exec.
            packed = subprocess.check_output(["bash", "-c", "find . -print0 | LC_ALL=C sort -z | cpio --null -o -H newc -R 0:0 --reproducible --quiet"], cwd=root)
            image = args.output / "guest-ssh.cpio.gz"
            image.write_bytes(gzip.compress(packed, compresslevel=9, mtime=0))
            image.chmod(0o600)
            report["image_sha256"] = digest(image)
            report["client_public_key_sha256"] = digest(client.with_suffix(".pub"))
            report["host_public_key_sha256"] = digest(args.output / "host-key.pub")
            report["success"] = True
    finally:
        (args.output / "build.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({"success": report["success"], "output": str(args.output)}))


if __name__ == "__main__":
    main()
