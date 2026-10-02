#!/usr/bin/env python3
"""Encrypted offline snapshot-store backup and verified recovery through S3.

Linux operators must stop every node using the store before backup. All those
nodes must implement the .backup.lock protocol; older nodes do not participate.
Credentials use boto3's standard provider chain; encryption uses a separate key.
"""
import argparse
import base64
import contextlib
import ctypes
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import stat
import struct
import sys
import tarfile
import tempfile
import uuid

MAGIC = b"HMBACK01"
MANIFEST = "HM_BACKUP_MANIFEST.json"
CHUNK = 1024 * 1024
MAX_OBJECT = 64 * 1024**3  # Ciphertext cap also stays below the GCM plaintext limit.
MAX_SINGLE_PUT = 5_000_000_000
MAX_FILES = 100_000
MAX_MANIFEST = 32 * 1024 * 1024
LOCK = ".backup.lock"


class UploadUncertain(Exception):
    def __init__(self, receipt, cleanup=None, interrupted=False):
        self.receipt = receipt
        self.cleanup = cleanup
        self.interrupted = interrupted
        super().__init__("upload not confirmed; object may already exist")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON field")
        result[key] = value
    return result


def read_json(data):
    return json.loads(data, object_pairs_hook=unique_object)


def safe_name(name):
    require(isinstance(name, str) and 0 < len(name.encode()) <= 4096, "invalid file name")
    path = PurePosixPath(name)
    require(not path.is_absolute() and all(p not in ["", ".", ".."] for p in name.split("/"))
            and "\\" not in name and "\0" not in name, "unsafe relative file name")
    require(path.as_posix() == name and name != LOCK, "reserved or noncanonical file name")
    return name


def read_key(path):
    with open(path, "rb") as stream:
        data = stream.read(129)
    require(len(data) <= 128, "backup key file is oversized")
    try:
        text = data.decode("ascii").strip()
        require(len(text) == 64 and all(c in "0123456789abcdefABCDEF" for c in text), "backup key must be 64 hex characters")
        return bytes.fromhex(text)
    except UnicodeError:
        raise ValueError("backup key must be 64 hex characters") from None


@contextlib.contextmanager
def offline_lock(root):
    # Never create this file: a missing lock indicates an unupgraded store.
    fd = os.open(root / LOCK, os.O_RDWR | os.O_NOFOLLOW)
    try:
        require(stat.S_ISREG(os.fstat(fd).st_mode), "store lock is not a regular file")
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError("stop every store node before offline backup") from None
        yield
    finally:
        os.close(fd)


def scan(root, maximum):
    files, directories, total = {}, {}, 0
    for parent, dirs, names in os.walk(root, followlinks=False):
        for name in sorted(dirs + names):
            path = Path(parent) / name
            relative = path.relative_to(root).as_posix()
            if relative == LOCK:
                continue
            safe_name(relative)
            info = path.lstat()
            require(not stat.S_ISLNK(info.st_mode), "store contains a symbolic link")
            require(not (relative.startswith("paused/") and (".claimed-" in name or name.startswith(".")))
                    and not (relative.startswith("templates/") and relative.split("/")[1].startswith(".")),
                    "store contains an unfinished claim or template build")
            if stat.S_ISDIR(info.st_mode):
                directories[relative] = info.st_mode & 0o777
            else:
                require(stat.S_ISREG(info.st_mode), "store contains a non-regular file")
                total += info.st_size
                require(total <= maximum, "store exceeds maximum expanded bytes")
                with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW), "rb") as stream:
                    require(os.fstat(stream.fileno()).st_size == info.st_size, "store changed during scan")
                    digest = hashlib.file_digest(stream, "sha256").hexdigest()
                files[relative] = {"size": info.st_size, "sha256": digest, "mode": info.st_mode & 0o777}
            require(len(files) + len(directories) <= MAX_FILES, "store contains too many entries")
    require(files, "store contains no backup data")
    return {"version": 1, "source_root": str(root), "files": files, "directories": directories,
            "expanded_bytes": total}


def snapshot_header(path):
    with open(path, "rb") as stream:
        prefix = stream.read(16)
        require(len(prefix) == 16 and prefix[:8] == b"HV2SNAP\0", "invalid snapshot magic")
        version, size = struct.unpack("<II", prefix[8:])
        require(version == 2 and size <= 1024 * 1024, "unsupported snapshot header")
        header = stream.read(size)
        require(len(header) == size, "truncated snapshot header")
        return read_json(header), 16 + size


def vm_snapshot(name):
    parts = PurePosixPath(name).parts
    return (parts[0] in ["paused", "snapshots"] and name.endswith(".snap")) or (
        len(parts) == 3 and parts[0] == "templates" and parts[-1] == "template.snap")


def dependencies(root, manifest):
    for name in manifest["files"]:
        if not vm_snapshot(name):
            continue
        header, _ = snapshot_header(root / name)
        for field in ["memory_base", "memory_image"]:
            value = header.get(field)
            if value is None:
                continue
            require(isinstance(value, str), "invalid snapshot memory reference")
            if field == "memory_image":
                require(not Path(value).is_absolute(), "snapshot memory image must be relative")
                safe_name(value)
            target = Path(value) if field == "memory_base" else (root / name).parent / value
            require(target.is_absolute(), "snapshot base reference must be absolute")
            resolved = target.resolve(strict=True)
            require(resolved.is_relative_to(root), "snapshot depends on a file outside the store")
            require(resolved.relative_to(root).as_posix() in manifest["files"], "snapshot memory dependency missing")


class DigestReader:
    def __init__(self, stream):
        self.stream, self.digest = stream, hashlib.sha256()

    def read(self, size):
        value = self.stream.read(size)
        self.digest.update(value)
        return value


def make_bundle(root, output, maximum):
    manifest = scan(root, maximum)
    dependencies(root, manifest)
    encoded = canonical(manifest)
    require(len(encoded) <= MAX_MANIFEST, "manifest is oversized")
    with tarfile.open(output, "w:gz", compresslevel=1) as archive:
        entry = tarfile.TarInfo(MANIFEST)
        entry.size, entry.mode = len(encoded), 0o600
        archive.addfile(entry, io.BytesIO(encoded))
        for name, record in sorted(manifest["files"].items()):
            with os.fdopen(os.open(root / name, os.O_RDONLY | os.O_NOFOLLOW), "rb") as stream:
                info = os.fstat(stream.fileno())
                require(stat.S_ISREG(info.st_mode) and info.st_size == record["size"], "store file changed during capture")
                reader = DigestReader(stream)
                entry = tarfile.TarInfo("store/" + name)
                entry.size, entry.mode = record["size"], 0o600
                archive.addfile(entry, reader)
                require(reader.digest.hexdigest() == record["sha256"], "store file changed during capture")
    require(scan(root, maximum) == manifest, "store changed during capture")
    return manifest


def encrypt(source, output, key):
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    require(source.stat().st_size + 36 <= MAX_OBJECT, "encrypted bundle exceeds format size limit")
    nonce = os.urandom(12)
    header = MAGIC + nonce
    cipher = Cipher(algorithms.AES(key), modes.GCM(nonce)).encryptor()
    cipher.authenticate_additional_data(header)
    with open(source, "rb") as src, open(output, "wb") as dst:
        dst.write(header)
        while block := src.read(CHUNK):
            dst.write(cipher.update(block))
        dst.write(cipher.finalize())
        dst.write(cipher.tag)
        dst.flush(); os.fsync(dst.fileno())
    require(output.stat().st_size <= MAX_OBJECT, "encrypted bundle exceeds format size limit")


def decrypt(source, output, key):
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    size = source.stat().st_size
    require(36 <= size <= MAX_OBJECT, "invalid encrypted backup length")
    with open(source, "rb") as src, open(output, "wb") as dst:
        header = src.read(20)
        require(header[:8] == MAGIC, "unsupported encrypted backup version")
        src.seek(-16, os.SEEK_END); tag = src.read(16); src.seek(20)
        cipher = Cipher(algorithms.AES(key), modes.GCM(header[8:], tag)).decryptor()
        cipher.authenticate_additional_data(header)
        remaining = size - 36
        while remaining:
            block = src.read(min(CHUNK, remaining))
            require(block, "truncated encrypted backup")
            dst.write(cipher.update(block)); remaining -= len(block)
        # Never parse or publish unauthenticated plaintext.
        dst.write(cipher.finalize())
        dst.flush(); os.fsync(dst.fileno())


def validate_manifest(manifest, maximum):
    require(manifest["version"] == 1 and isinstance(manifest["source_root"], str), "invalid backup manifest")
    require(Path(manifest["source_root"]).is_absolute(), "invalid original store root")
    files, directories = manifest["files"], manifest["directories"]
    require(isinstance(files, dict) and isinstance(directories, dict) and files, "invalid entry catalog")
    require(len(files) + len(directories) <= MAX_FILES and not files.keys() & directories.keys(), "invalid entry count")
    total = 0
    for name, value in files.items():
        safe_name(name)
        require(type(value["size"]) is int and 0 <= value["size"] <= maximum, "invalid file size")
        require(isinstance(value["sha256"], str) and len(value["sha256"]) == 64
                and all(c in "0123456789abcdef" for c in value["sha256"]), "invalid file digest")
        require(type(value["mode"]) is int and 0 <= value["mode"] <= 0o777, "invalid file permissions")
        total += value["size"]
    for name, mode in directories.items():
        safe_name(name)
        require(type(mode) is int and 0 <= mode <= 0o777, "invalid directory permissions")
    require(type(manifest["expanded_bytes"]) is int and total == manifest["expanded_bytes"] <= maximum, "expanded byte limit exceeded")
    for name in list(files) + list(directories):
        parent = PurePosixPath(name).parent
        while parent.as_posix() != ".":
            require(parent.as_posix() in directories, "entry parent missing or not a directory")
            parent = parent.parent


def extract_bundle(bundle, staging, maximum):
    with tarfile.open(bundle, "r|gz") as archive:
        first = archive.next()
        require(first is not None and first.name == MANIFEST and first.isfile() and first.size <= MAX_MANIFEST, "backup manifest missing or oversized")
        manifest = read_json(archive.extractfile(first).read(MAX_MANIFEST + 1))
        validate_manifest(manifest, maximum)
        for name in sorted(manifest["directories"], key=lambda n: (n.count("/"), n)):
            (staging / name).mkdir(mode=0o700)
        seen = set()
        while entry := archive.next():
            require(entry.isfile() and entry.name.startswith("store/"), "unsupported archive entry")
            name = safe_name(entry.name[6:])
            require(name in manifest["files"] and name not in seen, "extra or duplicate archive file")
            seen.add(name)
            record = manifest["files"][name]
            require(entry.size == record["size"], "archive file length mismatch")
            digest = hashlib.sha256()
            with archive.extractfile(entry) as src, open(staging / name, "xb") as dst:
                while block := src.read(CHUNK):
                    digest.update(block); dst.write(block)
                dst.flush(); os.fsync(dst.fileno())
            require(digest.hexdigest() == record["sha256"], "archive file digest mismatch")
        require(seen == set(manifest["files"]), "archive file missing")
    return manifest


def relocate(staging, destination, manifest):
    original = Path(manifest["source_root"])
    for name in manifest["files"]:
        if not vm_snapshot(name):
            continue
        header, offset = snapshot_header(staging / name)
        base = header.get("memory_base")
        if base is not None:
            require(isinstance(base, str) and Path(base).is_absolute(), "invalid snapshot base")
            try: relative = Path(base).relative_to(original).as_posix()
            except ValueError: raise ValueError("snapshot base escapes original store") from None
            require(relative in manifest["files"], "snapshot base missing from backup")
            header["memory_base"] = str(destination / relative)
            encoded = canonical(header)
            require(len(encoded) <= 1024 * 1024, "relocated snapshot header is oversized")
            temporary = staging / (name + ".relocating-" + uuid.uuid4().hex)
            require(not temporary.exists(), "reserved relocation file collision")
            with open(staging / name, "rb") as src, open(temporary, "xb") as dst:
                src.seek(offset)
                dst.write(b"HV2SNAP\0" + struct.pack("<II", 2, len(encoded)) + encoded)
                while block := src.read(CHUNK): dst.write(block)
                dst.flush(); os.fsync(dst.fileno())
            os.replace(temporary, staging / name)
        image = header.get("memory_image")
        if image is not None:
            require(isinstance(image, str) and not Path(image).is_absolute(), "invalid relative memory image")
            safe_name(image)
            require(((PurePosixPath(name).parent / image).as_posix()) in manifest["files"], "snapshot image missing from backup")
    for name, entry in manifest["files"].items(): os.chmod(staging / name, entry["mode"])
    for name, mode in sorted(manifest["directories"].items(), key=lambda kv: -kv[0].count("/")): os.chmod(staging / name, mode)
    # A restored store is immediately usable by upgraded nodes and future backups.
    fd = os.open(staging / LOCK, os.O_RDWR | os.O_CREAT | os.O_EXCL, 0o600)
    os.fsync(fd); os.close(fd)


def publish(staging, destination):
    # Atomic no-replace publication, including against a concurrent mkdir.
    libc = ctypes.CDLL(None, use_errno=True)
    rename = libc.renameat2
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(-100, os.fsencode(staging), -100, os.fsencode(destination), 1) != 0:
        raise OSError(ctypes.get_errno(), "could not atomically publish restored store")
    fd = os.open(destination.parent, os.O_RDONLY | os.O_DIRECTORY)
    try: os.fsync(fd)
    finally: os.close(fd)


def client(endpoint, region):
    import boto3
    from botocore.config import Config
    if endpoint is not None:
        from urllib.parse import urlsplit
        parsed = urlsplit(endpoint)
        require(parsed.scheme == "https" or (parsed.scheme == "http" and parsed.hostname in ["127.0.0.1", "localhost", "::1"]), "S3 endpoint requires HTTPS except loopback fixtures")
        require(parsed.username is None and parsed.password is None and not parsed.query and not parsed.fragment, "invalid S3 endpoint")
    return boto3.client("s3", endpoint_url=endpoint, region_name=region,
        config=Config(signature_version="s3v4", s3={"addressing_style": "path"},
                      connect_timeout=10, read_timeout=60, retries={"total_max_attempts": 1}))


def upload_ciphertext(s3, stream, bucket, object_key, receipt, threshold, part_size):
    size = receipt["encrypted_bytes"]
    receipt["upload_method"] = "multipart" if size >= threshold or size > MAX_SINGLE_PUT else "single"
    receipt["parts"] = 0
    if receipt["upload_method"] == "single":
        try:
            s3.put_object(Bucket=bucket, Key=object_key, Body=stream, ContentLength=size,
                          ContentType="application/octet-stream", IfNoneMatch="*",
                          ChecksumSHA256=base64.b64encode(bytes.fromhex(receipt["sha256"])).decode())
            receipt["parts"] = 1
            return
        except BaseException as error:
            raise UploadUncertain(receipt, interrupted=isinstance(error, KeyboardInterrupt)) from None
    upload_id = None
    try:
        result = s3.create_multipart_upload(Bucket=bucket, Key=object_key,
            ContentType="application/octet-stream", ChecksumAlgorithm="SHA256", ChecksumType="COMPOSITE")
        upload_id = result["UploadId"]
        require(isinstance(upload_id, str) and upload_id, "multipart upload ID missing")
        parts, composite = [], hashlib.sha256()
        while block := stream.read(part_size):
            number = len(parts) + 1
            require(number <= 10000, "multipart part limit exceeded")
            digest = hashlib.sha256(block).digest()
            checksum = base64.b64encode(digest).decode()
            result = s3.upload_part(Bucket=bucket, Key=object_key, UploadId=upload_id,
                PartNumber=number, Body=block, ContentLength=len(block), ChecksumSHA256=checksum)
            require(isinstance(result.get("ETag"), str) and result["ETag"], "multipart part receipt missing")
            if "ChecksumSHA256" in result:
                require(result["ChecksumSHA256"] == checksum, "multipart part checksum mismatch")
            composite.update(digest)
            parts.append({"ETag": result["ETag"], "PartNumber": number, "ChecksumSHA256": checksum})
            receipt["parts"] = number
        require(parts, "empty multipart ciphertext")
        checksum = base64.b64encode(composite.digest()).decode() + "-" + str(len(parts))
        receipt["composite_sha256"] = checksum
        result = s3.complete_multipart_upload(Bucket=bucket, Key=object_key, UploadId=upload_id,
            MultipartUpload={"Parts": parts}, ChecksumType="COMPOSITE", IfNoneMatch="*")
        if "ChecksumSHA256" in result:
            require(result["ChecksumSHA256"] == checksum, "completed multipart checksum mismatch")
    except BaseException as error:
        cleanup = {"status": "upload_id_unavailable"}
        if upload_id:
            # Only this invocation's upload is aborted. This never deletes an object,
            # even when completion committed before its acknowledgement was lost.
            cleanup["upload_id"] = upload_id
            try:
                s3.abort_multipart_upload(Bucket=bucket, Key=object_key, UploadId=upload_id)
                cleanup["status"] = "aborted"
            except BaseException as abort_error:
                code = getattr(abort_error, "response", {}).get("Error", {}).get("Code")
                cleanup["status"] = "already_completed_or_absent" if code == "NoSuchUpload" else "abort_failed"
        raise UploadUncertain(receipt, cleanup, isinstance(error, KeyboardInterrupt)) from None


def backup(args):
    root = args.store.resolve(strict=True)
    require(root.is_dir(), "store must be a directory")
    require(not args.key_file.resolve(strict=True).is_relative_to(root), "encryption key must be outside the store")
    key = read_key(args.key_file)
    s3 = client(args.endpoint, args.region)
    with offline_lock(root), tempfile.TemporaryDirectory(prefix="hm-backup-", dir=args.work_dir) as scratch:
        scratch = Path(scratch)
        require(not scratch.resolve().is_relative_to(root), "temporary work directory must be outside the store")
        bundle, encrypted = scratch / "bundle.tar.gz", scratch / "bundle.hmb"
        manifest = make_bundle(root, bundle, args.max_expanded_bytes)
        encrypt(bundle, encrypted, key)
        with encrypted.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").digest(); stream.seek(0)
            receipt = {"operation": "backup", "object": args.object, "encrypted_bytes": encrypted.stat().st_size,
                       "sha256": digest.hex(), "files": len(manifest["files"]), "expanded_bytes": manifest["expanded_bytes"]}
            upload_ciphertext(s3, stream, args.bucket, args.object, receipt,
                              args.multipart_threshold_mib * 1024**2, args.multipart_part_mib * 1024**2)
        return receipt


def restore(args):
    # Resolve the parent, not a supplied final symlink. Never replace a target.
    destination = args.destination.parent.resolve(strict=True) / args.destination.name
    require(not destination.exists() and not destination.is_symlink(), "restore destination already exists")
    key = read_key(args.key_file)
    s3 = client(args.endpoint, args.region)
    with tempfile.TemporaryDirectory(prefix="hm-restore-", dir=args.work_dir) as scratch:
        scratch = Path(scratch)
        encrypted, bundle = scratch / "bundle.hmb", scratch / "bundle.tar.gz"
        response = s3.get_object(Bucket=args.bucket, Key=args.object)
        length = response["ContentLength"]
        require(type(length) is int and 36 <= length <= MAX_OBJECT, "S3 object exceeds backup limit")
        with response["Body"] as body, encrypted.open("xb") as dst:
            count = 0
            while block := body.read(CHUNK):
                count += len(block); require(count <= length, "S3 body exceeds declared length"); dst.write(block)
            require(count == length, "S3 object truncated")
        if args.sha256 is not None:
            require(len(args.sha256) == 64 and all(c in "0123456789abcdef" for c in args.sha256), "expected checksum must be lowercase SHA-256 hex")
            with encrypted.open("rb") as stream:
                require(hashlib.file_digest(stream, "sha256").hexdigest() == args.sha256, "backup receipt checksum mismatch")
        decrypt(encrypted, bundle, key)
        # Staging is beside the destination so publication cannot cross filesystems.
        with tempfile.TemporaryDirectory(prefix=".hm-restore-", dir=destination.parent) as staging_name:
            staging = Path(staging_name)
            manifest = extract_bundle(bundle, staging, args.max_expanded_bytes)
            relocate(staging, destination, manifest)
            # Flush directory entries before publication. Files were synced individually.
            for parent, _, _ in os.walk(staging):
                fd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY)
                try: os.fsync(fd)
                finally: os.close(fd)
            publish(staging, destination)
        return {"operation": "restore", "object": args.object, "files": len(manifest["files"]),
                "expanded_bytes": manifest["expanded_bytes"], "memory_bases_relocated": True, "receipt_checksum_verified": args.sha256 is not None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ["backup", "restore"]:
        command = commands.add_parser(name)
        command.add_argument("--bucket", required=True)
        command.add_argument("--object", required=True, help="unique S3 object key; backup never overwrites")
        command.add_argument("--key-file", type=Path, required=True, help="separate 32-byte encryption key, encoded as 64 hex characters")
        command.add_argument("--endpoint", help="S3-compatible HTTPS endpoint; HTTP permitted only for loopback fixtures")
        command.add_argument("--region", default="us-east-1")
        command.add_argument("--work-dir", type=Path, help="private temporary files require capacity for compressed plaintext and ciphertext")
        command.add_argument("--max-expanded-bytes", type=int, default=64 * 1024**3)
        command.add_argument("--store" if name == "backup" else "--destination", type=Path, required=True)
        if name == "backup":
            command.add_argument("--multipart-threshold-mib", type=int, default=64, help="multipart upload threshold, 1 to 4096 MiB")
            command.add_argument("--multipart-part-mib", type=int, default=64, help="encrypted part buffer, 8 to 128 MiB")
        if name == "restore":
            command.add_argument("--sha256", help="independently retained receipt checksum; detects substitution of another valid backup")
    args = parser.parse_args()
    require(sys.platform == "linux", "offline backup v1 requires Linux file locks and renameat2")
    require(args.max_expanded_bytes > 0, "maximum expanded bytes must be positive")
    # All temporary files are private, including unauthenticated decryption output.
    os.umask(0o077)
    try:
        if args.command == "backup":
            require(1 <= args.multipart_threshold_mib <= 4096, "multipart threshold must be 1 to 4096 MiB")
            require(8 <= args.multipart_part_mib <= 128, "multipart part size must be 8 to 128 MiB")
        result = backup(args) if args.command == "backup" else restore(args)
        print(json.dumps(dict(result, success=True)))
        return 0
    except Exception as error:
        # SDK errors can contain operator endpoint/account information. Keep CLI
        # output credential-free; known validation errors carry fixed messages.
        message = str(error) if type(error) is ValueError else type(error).__name__
        failure = {"success": False, "operation": args.command, "error": message}
        if isinstance(error, UploadUncertain):
            failure["attempt_receipt"] = error.receipt
            failure["upload_confirmed"] = False
            if error.cleanup is not None: failure["multipart_cleanup"] = error.cleanup
            if error.interrupted: failure["interrupted"] = True
        print(json.dumps(failure), file=sys.stderr)
        return 130 if isinstance(error, UploadUncertain) and error.interrupted else 1


if __name__ == "__main__":
    raise SystemExit(main())
