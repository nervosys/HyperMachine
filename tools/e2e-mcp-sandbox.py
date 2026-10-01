#!/usr/bin/env python3
"""Check the shipped MCP stdio command with the official client and real guests."""
import argparse
import asyncio
import base64
from datetime import timedelta
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import uuid
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


async def check(args):
    known = []
    events = []
    errors = []
    key = os.environ.get("HV2_API_KEY", "")
    command = ["sandbox", "vm", "--endpoint", args.api_url, "mcp"]
    if args.envd_proxy:
        command.extend(["--envd-endpoint", args.envd_proxy, "--envd-domain", "sandbox.local"])
    params = StdioServerParameters(command=str(args.binary.resolve()),
        args=command,
        env={"HV2_API_KEY": key})
    async with stdio_client(params) as streams:
        async with ClientSession(*streams, read_timeout_seconds=timedelta(seconds=150)) as session:
            await session.initialize()
            async def call(name, arguments=None, failed=False):
                result = await session.call_tool(name, arguments or {})
                try:
                    value = json.loads(result.content[0].text)
                except json.JSONDecodeError:
                    value = {"error":result.content[0].text}
                # Capture every known creation before validating success so
                # partial forks can still be deleted in finally.
                if name == "sandbox_create" and isinstance(value, dict) and value.get("sandboxID"):
                    known.append(value["sandboxID"])
                if name == "sandbox_fork" and isinstance(value, list):
                    for entry in value:
                        if isinstance(entry, dict) and entry.get("sandbox", {}).get("sandboxID"):
                            known.append(entry["sandbox"]["sandboxID"])
                if bool(result.isError) != failed:
                    raise RuntimeError(f"unexpected error state: {name}")
                def no_tokens(item):
                    if isinstance(item, dict):
                        if "envdAccessToken" in item or "accessToken" in item:
                            raise RuntimeError("sandbox token exposed in tool result")
                        for child in item.values(): no_tokens(child)
                    elif isinstance(item, list):
                        for child in item: no_tokens(child)
                no_tokens(value)
                events.append(name)
                return value
            try:
                tools = await session.list_tools()
                expected_tools = 14 if args.envd_proxy else 12
                if len(tools.tools) != expected_tools:
                    raise RuntimeError(f"expected {expected_tools} shipped tools")
                parent = (await call("sandbox_create"))["sandboxID"]
                marker = "hm-mcp-" + uuid.uuid4().hex
                path = "/tmp/" + marker
                async def command(id, program, expected=None, failed=False):
                    value = await call("sandbox_exec", {"id":id,"command":["/bin/sh","-c",program]}, failed)
                    if expected is not None and value.get("stdout") != expected:
                        raise RuntimeError("guest output mismatch")
                    return value
                await command(parent, f"printf '%s' '{marker}' > '{path}'; cat '{path}'", marker)
                if args.envd_proxy:
                    payload = bytes(range(256)) * 1024
                    guest_path = "/root/mcp-binary'&query=literal.bin"
                    uploaded = await call("file_upload", {"id":parent,"path":guest_path,
                        "data_base64":base64.b64encode(payload).decode()})
                    if uploaded.get("bytes") != len(payload):
                        raise RuntimeError("file upload size mismatch")
                    downloaded = await call("file_download", {"id":parent,"path":guest_path})
                    if base64.b64decode(downloaded.get("data_base64", ""), validate=True) != payload:
                        raise RuntimeError("binary file roundtrip mismatch")
                    await command(parent, "head -c 262145 /dev/zero > /root/mcp-too-large.bin")
                    await call("file_download", {"id":parent,"path":"/root/mcp-too-large.bin"}, failed=True)
                info = await call("sandbox_inspect", {"id":parent})
                if info.get("cpuCount") != 1 or info.get("memoryMB") != 1024:
                    raise RuntimeError("expected 1 vCPU/1024 MiB fixture")
                listed = await call("sandbox_list")
                if not any(item.get("sandboxID") == parent for item in listed):
                    raise RuntimeError("created sandbox absent from list")
                await call("checkpoint_save", {"id":parent,"name":"before"})
                checkpoints = await call("checkpoint_list", {"id":parent})
                if not any(item.get("name") == "before" for item in checkpoints):
                    raise RuntimeError("checkpoint absent from list")
                await command(parent, f"printf changed > '{path}'")
                await call("checkpoint_restore", {"id":parent,"name":"before"})
                await command(parent, f"cat '{path}'", marker)
                await call("checkpoint_delete", {"id":parent,"name":"before"})
                await call("sandbox_pause", {"id":parent})
                await call("sandbox_resume", {"id":parent})
                await command(parent, f"cat '{path}'", marker)
                forks = await call("sandbox_fork", {"id":parent})
                if len(forks) != 1 or "sandbox" not in forks[0]:
                    raise RuntimeError("fork did not produce one child")
                child = known[-1]
                if child == parent: raise RuntimeError("fork reused parent ID")
                await command(child, f"cat '{path}'", marker)
                await command(child, f"printf child > '{path}'")
                await command(parent, f"cat '{path}'", marker)
                failed = await command(parent, "exit 7", failed=True)
                if failed.get("exit_code") != 7: raise RuntimeError("guest exit status lost")
                await call("sandbox_inspect", {"id":"missing-mcp-fixture"}, failed=True)
            except Exception as error:
                errors.append(str(error).replace(key, "[redacted]") if key else str(error))
            finally:
                for id in list(known):
                    try:
                        await call("sandbox_delete", {"id":id})
                        known.remove(id)
                    except Exception:
                        errors.append("known sandbox cleanup failed")
    report = {"schema_version":1,"client":"official-mcp-python",
        "client_version":importlib.metadata.version("mcp"),
        "harness_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "binary_sha256":hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "environment":args.environment,"operations":events,"errors":errors,
        "remaining_known_sandboxes":known,"success":not errors and not known}
    print(json.dumps(report, indent=2))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--api-url", required=True)
    parser.add_argument("--environment", required=True)
    parser.add_argument("--envd-proxy", help="Enable binary file tools through this operator-selected proxy")
    raise SystemExit(asyncio.run(check(parser.parse_args())))
