"""Check that a guest agent is alive and answering, over a real AF_VSOCK socket.

Run this from the host against a booted guest, or inside the guest itself, when
bringing up an image. It exercises what unit tests cannot: the framing, the
daemon read loop, and the exec path, through the kernel rather than through a
fake channel in the same process.

    # inside the guest, or anywhere that can reach its CID
    python3 vsock_probe.py <cid>        # default 1, the local CID

A local run needs the loopback transport, which is a module on most kernels:

    sudo modprobe vsock_loopback

Exits non-zero and prints what failed if the agent is missing, mismatched, or
not answering -- which are the three things worth telling apart when a guest
image looks fine and `vm.exec` still times out.
"""
import json
import socket
import struct
import sys

VMADDR_CID_LOCAL = 1
PORT = 1024
PROTOCOL_VERSION = 1


def frame(obj):
    body = json.dumps(obj).encode()
    return struct.pack("<I", len(body)) + body


def read_frame(sock, timeout=10.0):
    sock.settimeout(timeout)
    buf = b""
    while len(buf) < 4:
        chunk = sock.recv(4 - len(buf))
        if not chunk:
            raise RuntimeError("connection closed before a length arrived")
        buf += chunk
    (length,) = struct.unpack("<I", buf)
    body = b""
    while len(body) < length:
        chunk = sock.recv(length - len(body))
        if not chunk:
            raise RuntimeError("connection closed mid-frame")
        body += chunk
    return json.loads(body)


def main():
    cid = int(sys.argv[1]) if len(sys.argv) > 1 else VMADDR_CID_LOCAL
    sock = socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM)
    try:
        sock.settimeout(5.0)
        sock.connect((cid, PORT))
    except OSError as e:
        print("CONNECT FAILED (cid=%d port=%d): %s" % (cid, PORT, e))
        return 2
    print("connected to cid=%d port=%d" % (cid, PORT))

    # 1. Ping — the cheapest proof anything is listening in there.
    sock.sendall(frame({"id": 1, "version": PROTOCOL_VERSION, "op": {"kind": "ping"}}))
    pong = read_frame(sock)
    print("ping  ->", json.dumps(pong))
    assert pong["id"] == 1, pong
    assert pong["result"]["kind"] == "pong", pong

    # 2. Exec — a real program, run by the daemon, output returned over vsock.
    sock.sendall(
        frame(
            {
                "id": 2,
                "version": PROTOCOL_VERSION,
                "op": {
                    "kind": "exec",
                    "program": "/bin/sh",
                    "args": ["-c", "echo hello-from-guest; uname -s; exit 7"],
                    "timeout_ms": 5000,
                },
            }
        )
    )
    out = read_frame(sock)
    print("exec  ->", json.dumps(out))
    assert out["id"] == 2, out
    r = out["result"]
    assert r["kind"] == "exited", r
    assert "hello-from-guest" in r["stdout"], r
    assert r["exit_code"] == 7, "a non-zero exit must be reported as itself: %r" % r

    # 3. Version mismatch — the guard against an old agent meeting a new host.
    sock.sendall(frame({"id": 3, "version": 999, "op": {"kind": "ping"}}))
    bad = read_frame(sock)
    print("v999  ->", json.dumps(bad))
    assert bad["result"]["kind"] == "failed", bad

    sock.close()
    print("ALL CHECKS PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
