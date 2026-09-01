"""The guest half of the resident runtime.

Read length-prefixed JSON frames from stdin, execute each one's code in a
namespace that outlives the call, and write back what it printed. The namespace
surviving is the whole point: a value computed in one frame is still a value in
the next, which is what a fresh process per call cannot do.

Placeholders (__MAX_FRAME__, __MAX_OUTPUT__, __MEMORY_BYTES__) are substituted
by the host before this is written out, so both ends agree on the ceilings
without either having to be told at runtime by the other.
"""

import contextlib
import io
import json
import struct
import sys
import traceback

VERSION = 1
MAX_FRAME = __MAX_FRAME__
MAX_OUTPUT = __MAX_OUTPUT__
MEMORY_BYTES = __MEMORY_BYTES__

# What this process could actually apply to itself, reported to the host in the
# ready frame. The host asks for confinement; only this end can find out
# whether the kernel granted it, and a host that assumed would be claiming a
# limit that may not exist.
applied = []

if MEMORY_BYTES:
    try:
        import resource

        soft, hard = resource.getrlimit(resource.RLIMIT_AS)
        want = MEMORY_BYTES
        if hard != resource.RLIM_INFINITY:
            want = min(want, hard)
        # Both soft and hard: lowering a hard limit is irreversible for an
        # unprivileged process, so code arriving in a later frame cannot raise
        # it back. Done before any of that code runs.
        resource.setrlimit(resource.RLIMIT_AS, (want, want))
        applied.append("memory")
    except Exception:
        # No `resource` module (Windows), or the kernel refused. Saying nothing
        # is the honest answer; the host reports the control as unenforced.
        pass

stdin = sys.stdin.buffer
stdout = sys.stdout.buffer


def send(obj):
    body = json.dumps(obj).encode("utf-8")
    stdout.write(struct.pack("<I", len(body)))
    stdout.write(body)
    stdout.flush()


def read_exact(count):
    buf = b""
    while len(buf) < count:
        chunk = stdin.read(count - len(buf))
        if not chunk:
            return None
        buf += chunk
    return buf


def clip(text):
    data = text.encode("utf-8")
    if len(data) <= MAX_OUTPUT:
        return text, False
    return data[:MAX_OUTPUT].decode("utf-8", "ignore"), True


send(
    {
        "version": VERSION,
        "reply": {
            "kind": "ready",
            "python": sys.version.split()[0],
            "applied": applied,
        },
    }
)

# One namespace, globals and locals both, for every frame this process ever
# handles. That identity is what makes `x = 1` in one call visible to the next.
namespace = {"__name__": "__context__"}

while True:
    header = read_exact(4)
    if header is None:
        break
    (length,) = struct.unpack("<I", header)
    if length > MAX_FRAME:
        # The length is written by the other end. Refuse before allocating.
        sys.stderr.write(
            "frame of %d bytes exceeds the %d-byte limit\n" % (length, MAX_FRAME)
        )
        sys.exit(2)
    body = read_exact(length)
    if body is None:
        break

    request = json.loads(body.decode("utf-8"))
    out, err = io.StringIO(), io.StringIO()
    ok = True
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            exec(compile(request["code"], "<context>", "exec"), namespace, namespace)
    except BaseException:
        # BaseException, so a sys.exit() in agent-written code ends that call
        # rather than the namespace every later call depends on.
        ok = False
        err.write(traceback.format_exc())

    text_out, cut_out = clip(out.getvalue())
    text_err, cut_err = clip(err.getvalue())
    send(
        {
            "version": VERSION,
            "reply": {
                "kind": "result",
                "id": request["id"],
                "ok": ok,
                "stdout": text_out,
                "stderr": text_err,
                "truncated": cut_out or cut_err,
            },
        }
    )
