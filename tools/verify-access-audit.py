#!/usr/bin/env python3
"""Independently verify protected control-plane audit records with an operator key."""
import argparse
import hashlib
import hmac
import json
from pathlib import Path
import struct


def verify(data, key, sandbox_id=None):
    if len(key) != 32:
        raise ValueError("audit key must be 32 bytes")
    if data and not data.endswith(b"\n"):
        raise ValueError("incomplete audit tail")
    def require(condition, message):
        if not condition:
            raise ValueError(message)
    def unique_object(pairs):
        result = {}
        for name, value in pairs:
            require(name not in result, "duplicate JSON field")
            result[name] = value
        return result
    target = None
    target_counts = {"admissions": 0, "completions": 0, "completion_statuses": {}}
    if sandbox_id is not None:
        encoded = sandbox_id.encode()
        require(0 < len(encoded) <= 256, "sandbox ID must contain 1 to 256 UTF-8 bytes")
        target = hmac.new(key, b"HyperMachine access resource v1\0sandbox\0" + encoded, hashlib.sha256).hexdigest()
    previous = bytes(32)
    pending = {}
    statuses = {}
    count = 0
    def integer_numbers(value):
        if isinstance(value, float):
            raise ValueError("control-plane audit schema requires integer numeric fields")
        if isinstance(value, dict):
            for item in value.values(): integer_numbers(item)
        if isinstance(value, list):
            for item in value: integer_numbers(item)
    for line in data.splitlines():
        record = json.loads(line, object_pairs_hook=unique_object)
        require(type(record["seq"]) is int and type(record["timestamp_ms"]) is int, "non-integer record sequence or timestamp")
        require(0 <= record["timestamp_ms"] < 2**64, "invalid audit timestamp")
        require(record["seq"] == count, "audit sequence mismatch")
        require(record["source"] == "control-plane-access", "unexpected audit source")
        require(bytes.fromhex(record["prev"]) == previous, "audit predecessor mismatch")
        integer_numbers(record["event"])
        source = record["source"].encode()
        canonical = json.dumps(record["event"], sort_keys=True, ensure_ascii=False,
            separators=(",", ":"), allow_nan=False).encode()
        message = (b"HyperMachine audit chain v1\0" + struct.pack(">QQI", record["seq"],
            record["timestamp_ms"], len(source)) + source + previous + canonical)
        calculated = hmac.new(key, message, hashlib.sha256).digest()
        require(hmac.compare_digest(calculated, bytes.fromhex(record["mac"])), "audit MAC mismatch")
        previous = calculated
        event = record["event"]
        reference = event.get("sandbox_ref")
        require("sandbox_ref" not in event or (isinstance(reference, str) and len(reference) == 64 and all(c in "0123456789abcdef" for c in reference)), "invalid sandbox reference")
        identity = (reference, event["route"], event["method"], event["principal"], event["key_id"], event["allowed"])
        if event["phase"] == "admission":
            require(event["request_id"] not in pending, "duplicate admission")
            pending[event["request_id"]] = identity
        else:
            require(event["phase"] == "completion", "unknown audit phase")
            require(pending.pop(event["request_id"]) == identity, "completion identity mismatch")
            statuses[str(event["status"])] = statuses.get(str(event["status"]), 0) + 1
        if target is not None and reference == target:
            if event["phase"] == "admission": target_counts["admissions"] += 1
            else:
                target_counts["completions"] += 1
                status = str(event["status"])
                target_counts["completion_statuses"][status] = target_counts["completion_statuses"].get(status, 0) + 1
        count += 1
    result = {"verified_records": count, "uncompleted_admissions": len(pending), "completion_statuses": statuses}
    if target is not None: result["sandbox_target"] = dict(target_counts, reference=target)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("--key-file", type=Path, required=True)
    parser.add_argument("--sandbox-id", help="count records targeting this sandbox without printing its raw ID")
    args = parser.parse_args()
    print(json.dumps(verify(args.log.read_bytes(), bytes.fromhex(args.key_file.read_text().strip()), args.sandbox_id)))


if __name__ == "__main__":
    main()
