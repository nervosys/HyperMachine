#!/usr/bin/env python3
"""Independently verify protected control-plane audit records with an operator key."""
import argparse
import hashlib
import hmac
import json
from pathlib import Path
import struct


def verify(data, key):
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
        identity = (event["route"], event["method"], event["principal"], event["key_id"], event["allowed"])
        if event["phase"] == "admission":
            require(event["request_id"] not in pending, "duplicate admission")
            pending[event["request_id"]] = identity
        else:
            require(event["phase"] == "completion", "unknown audit phase")
            require(pending.pop(event["request_id"]) == identity, "completion identity mismatch")
            statuses[str(event["status"])] = statuses.get(str(event["status"]), 0) + 1
        count += 1
    return {"verified_records": count, "uncompleted_admissions": len(pending), "completion_statuses": statuses}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("--key-file", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(verify(args.log.read_bytes(), bytes.fromhex(args.key_file.read_text().strip()))))


if __name__ == "__main__":
    main()
