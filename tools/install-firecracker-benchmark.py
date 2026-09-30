#!/usr/bin/env python3
"""Install the locked official x86_64 Firecracker binary for local benchmarks."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import platform
import subprocess
import tarfile
import time
import urllib.request

VERSION = "1.17.0"
ARCHIVE_SHA256 = "06094a1108ae9e82aa4c23a775aa92758f53f1175d422270d9d6162cb9ade558"
URL = f"https://github.com/firecracker-microvm/firecracker/releases/download/v{VERSION}/firecracker-v{VERSION}-x86_64.tgz"
LIMIT = 16 * 1024 * 1024


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("this benchmark installer requires Linux x86_64")
    if args.archive:
        with args.archive.open("rb") as source:
            data = source.read(LIMIT + 1)
    else:
        for attempt in range(3):
            try:
                with urllib.request.urlopen(URL, timeout=30) as source:
                    data = source.read(LIMIT + 1)
                break
            except OSError:
                if attempt == 2: raise
                time.sleep(attempt + 1)
    if len(data) > LIMIT or hashlib.sha256(data).hexdigest() != ARCHIVE_SHA256:
        raise RuntimeError("official Firecracker archive checksum/size mismatch")
    name = f"firecracker-v{VERSION}-x86_64"
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        candidates = [item for item in archive.getmembers() if Path(item.name).name == name]
        if len(candidates) != 1 or not candidates[0].isfile() or candidates[0].size > LIMIT:
            raise RuntimeError("unexpected Firecracker archive member")
        binary = archive.extractfile(candidates[0]).read(LIMIT + 1)
    if len(binary) != candidates[0].size:
        raise RuntimeError("incomplete Firecracker binary")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    cached_archive = args.output_dir / f"firecracker-v{VERSION}-x86_64.tgz"
    if not cached_archive.exists():
        with cached_archive.open("xb") as output:
            output.write(data)
    destination = args.output_dir / name
    if destination.exists():
        if destination.read_bytes() != binary:
            raise RuntimeError("refusing to overwrite a different existing binary")
    else:
        with destination.open("xb") as output:
            output.write(binary)
    destination.chmod(0o755)
    version_output = subprocess.check_output([str(destination.resolve()), "--version"], text=True, timeout=10)
    # This release also writes a timestamped clean-exit log to stdout.
    version = version_output.splitlines()[0].strip()
    if version != f"Firecracker v{VERSION}":
        raise RuntimeError("installed version mismatch")
    print(json.dumps({"version":version,"source_url":URL,"archive_sha256":ARCHIVE_SHA256,
        "binary_sha256":hashlib.sha256(binary).hexdigest(),"binary":str(destination.resolve())}))


if __name__ == "__main__":
    main()
