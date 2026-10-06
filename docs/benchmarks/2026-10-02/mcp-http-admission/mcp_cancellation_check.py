"""Real-backend MCP cancellation check using the official client's public APIs."""
import asyncio
import json
import time
import urllib.request
import uuid

from mcp.types import CancelledNotification, CancelledNotificationParams, ClientNotification


class ObservedWrite:
    """Observe an outbound protocol ID without accessing ClientSession internals."""
    def __init__(self, stream):
        self.stream = stream
        self.command = None
        self.request_id = None
        self.sent = asyncio.Event()

    def __getattr__(self, name):
        return getattr(self.stream, name)

    async def __aenter__(self):
        await self.stream.__aenter__()
        return self

    async def __aexit__(self, *args):
        return await self.stream.__aexit__(*args)

    async def send(self, message):
        request = message.message.root
        matches = (getattr(request, "method", None) == "tools/call"
            and (request.params or {}).get("name") == "sandbox_exec"
            and (request.params or {}).get("arguments", {}).get("command") == self.command)
        await self.stream.send(message)
        if matches:
            self.request_id = request.id
            self.sent.set()


async def check_cancellation(session, writer, api_url, sandbox, key):
    marker = "/tmp/mcp-cancellation-" + uuid.uuid4().hex
    start, finish = marker + "-start", marker + "-finish"
    writer.command = ["/bin/sh", "-c",
        f"printf started > '{start}'; sleep 5; printf finished > '{finish}'"]
    pending = asyncio.create_task(session.call_tool("sandbox_exec",
        {"id":sandbox,"command":writer.command,"timeout":20}))

    def command(program):
        headers = {"Content-Type":"application/json"}
        if key: headers["X-API-Key"] = key
        request = urllib.request.Request(api_url.rstrip("/") + f"/sandboxes/{sandbox}/exec",
            data=json.dumps({"cmd":program,"timeout_secs":2}).encode(), headers=headers, method="POST")
        with urllib.request.urlopen(request, timeout=5) as response:
            result = json.load(response)
        if result.get("exit_code") != 0 or result.get("timed_out"):
            raise RuntimeError("cancellation marker probe failed")
        return result.get("stdout", "")

    async def marker_value(path, expected, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = await asyncio.to_thread(command, f"cat '{path}' 2>/dev/null || true")
            if value == expected: return
            await asyncio.sleep(.02)
        raise RuntimeError("cancellation marker deadline exceeded")

    try:
        await asyncio.wait_for(writer.sent.wait(), 5)
        await marker_value(start, "started", 5)
        if pending.done(): raise RuntimeError("request completed before cancellation")
        await session.send_notification(ClientNotification(CancelledNotification(
            params=CancelledNotificationParams(requestId=writer.request_id, reason="fixture cancellation"))))
        started = time.perf_counter()
        await asyncio.wait_for(session.send_ping(), 2)
        ping_ms = (time.perf_counter() - started) * 1000
        if pending.done(): raise RuntimeError("cancelled request received a response")
        # Cancelling this client wait cannot undo accepted /exec work.
        await marker_value(finish, "finished", 10)
        if pending.done(): raise RuntimeError("cancelled request received a late response")
        return {"success":True,"backend_accepted_before_cancel":True,
            "backend_completed_after_cancel":True,"cancelled_request_received_response":False,
            "session_ping_after_cancel_ms":ping_ms,
            "scope":"MCP client wait cancelled; accepted remote guest work continues"}
    finally:
        pending.cancel()
        try: await pending
        except asyncio.CancelledError: pass
