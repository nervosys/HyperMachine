#!/usr/bin/env python3
"""Install checksum-locked protoc 23.4 without GitHub release enumeration."""
import argparse
import hashlib
import io
import os
from pathlib import Path, PurePosixPath
import platform
import subprocess
import tempfile
import time
import urllib.request
import zipfile

VERSION = "23.4"
HASHES = {
    "linux-x86_64": "0502f286ac9ed860b629a7965a14527b1f2dd131e4283fa23c2d7f184672aa9a",
    "linux-aarch_64": "1c7750b6e038305b5a7fc3d0cda1ebefdf106a4f30a787bf826ed2fc47c3967d",
    "osx-x86_64": "07e5fdcf1b0708d3367dc5e6eb8d135de7e407d75316c93155cfd8ab362eec80",
    "osx-aarch_64": "8c7afae8626b6811e7b5897d16d940c2dbf50b1e135ed958a01db6566bdda726",
    "win64": "a309c39442fb75f0db343cb22c111a00f91cdf0767f332e170644b9378e2bcc6",
}


def target():
    system, machine = platform.system(), platform.machine().lower()
    arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch_64", "aarch64": "aarch_64"}.get(machine)
    if system == "Windows" and arch == "x86_64":
        return "win64"
    prefix = {"Linux": "linux", "Darwin": "osx"}.get(system)
    key = f"{prefix}-{arch}"
    if key not in HASHES:
        raise RuntimeError(f"unsupported protoc platform: {system}/{machine}")
    return key


def verify(data, key):
    if hashlib.sha256(data).hexdigest() != HASHES[key]:
        raise RuntimeError("protoc archive checksum mismatch")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="Verify and install an already downloaded official archive")
    args = parser.parse_args()
    key = target()
    if args.archive:
        data = args.archive.read_bytes()
    else:
        url = f"https://github.com/protocolbuffers/protobuf/releases/download/v{VERSION}/protoc-{VERSION}-{key}.zip"
        for attempt in range(3):
            try:
                with urllib.request.urlopen(url, timeout=30) as response:
                    data = response.read(16 * 1024 * 1024 + 1)
                if len(data) > 16 * 1024 * 1024:
                    raise RuntimeError("protoc archive exceeds size limit")
                break
            except OSError:
                if attempt == 2:
                    raise
                time.sleep(attempt + 1)
    verify(data, key)
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        for name in archive.namelist():
            path = PurePosixPath(name)
            if path.is_absolute() or ".." in path.parts or "\\" in name or ":" in name:
                raise RuntimeError("unsafe protoc archive path")
        destination = Path(tempfile.mkdtemp(prefix="hm-protoc-", dir=os.environ.get("RUNNER_TEMP")))
        archive.extractall(destination)
    binary = destination / "bin" / ("protoc.exe" if key == "win64" else "protoc")
    if key != "win64":
        binary.chmod(0o755)
    result = subprocess.run([str(binary), "--version"], check=True, capture_output=True, text=True, timeout=10)
    if result.stdout.strip() != f"libprotoc {VERSION}":
        raise RuntimeError("unexpected installed protoc version")
    if os.environ.get("GITHUB_PATH"):
        with open(os.environ["GITHUB_PATH"], "a", encoding="utf-8") as output:
            output.write(str(binary.parent) + "\n")
    print(f"Verified {result.stdout.strip()} at {binary.parent}")


if __name__ == "__main__":
    main()
