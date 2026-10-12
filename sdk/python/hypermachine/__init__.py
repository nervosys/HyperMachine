"""A client for HyperMachine's machine API, in Python, with no dependencies.

    from hypermachine import Client

    hm = Client("http://node:3980", api_key="...")
    web = hm.machines.create("web-01", cpu_count=2, memory_mb=2048, disk_gib=20)
    print(web.exec("uname -a").stdout)

    run = web.runs.start("make test", cwd="/srv/app")
    for chunk in run.follow():
        print(chunk, end="")

    web.files.write("/etc/app/config.toml", b"port = 8080\\n")
    web.stop()
    snap = web.snapshots.take("before upgrade")
    print(snap.read("/etc/hostname"))

Every call is one HTTP request to a node or a control plane, which answer
the same routes. A refusal raises `ApiError` with the status and the node's
own words. Nothing is cached: an object's attributes are what the node said
when it was fetched.

The routes are documented in docs/MACHINES.md, docs/USAGE.md and
docs/ENVIRONMENTS.md.
"""

from __future__ import annotations

import base64
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, Iterator, List, Mapping, Optional

__all__ = [
    "ApiError",
    "Client",
    "ExecResult",
    "Machine",
    "Prompt",
    "Run",
    "Snapshot",
]

__version__ = "0.1.0"


class ApiError(Exception):
    """The node refused, or could not do, what was asked."""

    def __init__(self, status: int, message: str, method: str, path: str):
        super().__init__(f"{method} {path}: {status} {message}")
        self.status = status
        self.message = message


class Client:
    """A node or a control plane.

    `endpoint` defaults to `HV2_SANDBOX_URL`, then `http://127.0.0.1:3980`;
    `api_key` to `HV2_API_KEY`. The key is sent as `x-api-key` and never
    anywhere else.
    """

    def __init__(
        self,
        endpoint: Optional[str] = None,
        api_key: Optional[str] = None,
        timeout: float = 120.0,
    ):
        self.endpoint = (
            endpoint or os.environ.get("HV2_SANDBOX_URL") or "http://127.0.0.1:3980"
        ).rstrip("/")
        self.api_key = api_key if api_key is not None else os.environ.get("HV2_API_KEY")
        self.timeout = timeout
        self.machines = Machines(self)

    def __repr__(self) -> str:
        # Not the key.
        return f"Client({self.endpoint!r})"

    # The HTTP underneath everything.

    def _url(self, path: str, query: Optional[Mapping[str, Any]] = None) -> str:
        url = self.endpoint + path
        if query:
            pairs = {k: v for k, v in query.items() if v is not None}
            if pairs:
                url += "?" + urllib.parse.urlencode(pairs)
        return url

    def raw(
        self,
        method: str,
        path: str,
        body: Optional[bytes] = None,
        query: Optional[Mapping[str, Any]] = None,
        content_type: Optional[str] = None,
        timeout: Optional[float] = None,
    ):
        """One request; the open response. Raises `ApiError` on a refusal."""
        request = urllib.request.Request(self._url(path, query), data=body, method=method)
        if content_type:
            request.add_header("content-type", content_type)
        if self.api_key:
            request.add_header("x-api-key", self.api_key)
        try:
            return urllib.request.urlopen(request, timeout=timeout or self.timeout)
        except urllib.error.HTTPError as error:
            raw = error.read()
            try:
                message = json.loads(raw).get("message") or raw.decode(errors="replace")
            except (ValueError, AttributeError):
                message = raw.decode(errors="replace")
            raise ApiError(error.code, message, method, path) from None

    def call(
        self,
        method: str,
        path: str,
        body: Any = None,
        query: Optional[Mapping[str, Any]] = None,
        timeout: Optional[float] = None,
    ) -> Any:
        """One JSON request; the JSON answer, or None for an empty one."""
        data = None if body is None else json.dumps(body).encode()
        with self.raw(
            method,
            path,
            data,
            query,
            "application/json" if data is not None else None,
            timeout,
        ) as response:
            raw = response.read()
        return json.loads(raw) if raw.strip() else None

    # Across machines.

    def usage(self, start: Any = None, end: Any = None, by: str = "sandbox") -> Dict[str, Any]:
        """Running time, vCPU-seconds and GiB-seconds of the caller's
        sandboxes in a window; times are RFC 3339 text or epoch
        milliseconds. See docs/USAGE.md."""
        return self.call("GET", "/usage", query={"from": start, "to": end, "by": by})

    def put_environment(
        self,
        name: str,
        env_vars: Optional[Mapping[str, str]] = None,
        files: Optional[Mapping[str, bytes]] = None,
    ) -> Dict[str, Any]:
        """Make or replace a team's environment: variables, and files by
        path. See docs/ENVIRONMENTS.md."""
        body = {
            "envVars": dict(env_vars or {}),
            "files": [
                {"path": path, "contentBase64": base64.b64encode(data).decode()}
                for path, data in (files or {}).items()
            ],
        }
        return self.call("PUT", f"/environments/{_seg(name)}", body)

    def environments(self) -> List[Dict[str, Any]]:
        return self.call("GET", "/environments")

    def delete_environment(self, name: str) -> None:
        self.call("DELETE", f"/environments/{_seg(name)}")


def _seg(part: Any) -> str:
    """One path segment, so a name can never step into another route."""
    return urllib.parse.quote(str(part), safe="")


@dataclass
class ExecResult:
    exit_code: Optional[int]
    signal: Optional[int]
    stdout: str
    stderr: str


class Machines:
    """`client.machines`."""

    def __init__(self, client: Client):
        self._client = client

    def create(
        self,
        name: str,
        template: Optional[str] = None,
        image: Optional[str] = None,
        cpu_count: Optional[int] = None,
        memory_mb: Optional[int] = None,
        disk_gib: Optional[int] = None,
        start: bool = True,
        guest_agent: Optional[bool] = None,
        user_data: Optional[str] = None,
        network: Optional[Mapping[str, Any]] = None,
        **more: Any,
    ) -> "Machine":
        """A new machine, from a template (the node's `base` unless given)
        or from a disk image in the node's image directory."""
        body: Dict[str, Any] = {"name": name, "start": start}
        for key, value in [
            ("templateID", template),
            ("image", image),
            ("cpuCount", cpu_count),
            ("memoryMB", memory_mb),
            ("diskGiB", disk_gib),
            ("guestAgent", guest_agent),
            ("userData", user_data),
            ("network", network),
        ]:
            if value is not None:
                body[key] = value
        body.update(more)
        return Machine(self._client, self._client.call("POST", "/machines", body))

    def list(self) -> List["Machine"]:
        return [Machine(self._client, m) for m in self._client.call("GET", "/machines")]

    def get(self, name: str) -> "Machine":
        return Machine(self._client, self._client.call("GET", f"/machines/{_seg(name)}"))


class Machine:
    """One machine, as the node described it when it was fetched."""

    def __init__(self, client: Client, described: Mapping[str, Any]):
        self._client = client
        self._set(described)
        self.runs = Runs(self)
        self.prompts = Prompts(self)
        self.files = Files(self)
        self.ports = Ports(self)
        self.snapshots = Snapshots(self)

    def _set(self, described: Mapping[str, Any]) -> None:
        self.info: Dict[str, Any] = dict(described)
        self.name: str = described["name"]
        self.id: str = described.get("machineID", "")
        self.state: str = described.get("state", "")

    def __repr__(self) -> str:
        return f"Machine({self.name!r}, state={self.state!r})"

    @property
    def _path(self) -> str:
        return f"/machines/{_seg(self.name)}"

    def refresh(self) -> "Machine":
        self._set(self._client.call("GET", self._path))
        return self

    def _act(self, action: str) -> "Machine":
        self._set(self._client.call("POST", f"{self._path}/{action}", timeout=600))
        return self

    def start(self) -> "Machine":
        return self._act("start")

    def stop(self) -> "Machine":
        return self._act("stop")

    def restart(self) -> "Machine":
        return self._act("restart")

    def delete(self) -> None:
        self._client.call("DELETE", self._path)

    def resize(
        self,
        cpu_count: Optional[int] = None,
        memory_mb: Optional[int] = None,
        disk_gib: Optional[int] = None,
    ) -> "Machine":
        """Another size for a stopped machine, there at its next start."""
        body = {
            key: value
            for key, value in [("cpuCount", cpu_count), ("memoryMB", memory_mb), ("diskGiB", disk_gib)]
            if value is not None
        }
        self._set(self._client.call("PATCH", self._path, body))
        return self

    def exec(self, cmd: str, timeout: int = 60) -> ExecResult:
        """Run a shell line in the guest and wait for it."""
        answer = self._client.call(
            "POST", f"{self._path}/exec", {"cmd": cmd, "timeout_secs": timeout}, timeout=timeout + 30
        )
        return ExecResult(
            answer.get("exit_code"), answer.get("signal"), answer.get("stdout", ""), answer.get("stderr", "")
        )


class Run:
    """A command left running in a machine."""

    def __init__(self, machine: Machine, described: Mapping[str, Any]):
        self.machine = machine
        self.info: Dict[str, Any] = dict(described)
        self.id: int = described["runID"]

    def __repr__(self) -> str:
        return f"Run({self.id}, state={self.info.get('state')!r})"

    @property
    def _path(self) -> str:
        return f"{self.machine._path}/runs/{self.id}"

    @property
    def state(self) -> str:
        return self.info.get("state", "")

    @property
    def exit_code(self) -> Optional[int]:
        return self.info.get("exitCode")

    def read(self, stdout: int = 0, stderr: int = 0) -> Dict[str, Any]:
        """The run, and what it printed from these byte offsets on."""
        answer = self.machine._client.call("GET", self._path, query={"stdout": stdout, "stderr": stderr})
        self.info = answer
        return answer

    def follow(self, interval: float = 0.5) -> Iterator[str]:
        """Its standard output as it arrives, until it ends."""
        offset = 0
        while True:
            answer = self.read(stdout=offset)
            text = answer["stdout"]["text"]
            if text:
                yield text
            offset = answer["stdout"]["next"]
            if answer["state"] != "running":
                return
            time.sleep(interval)

    def wait(self, interval: float = 0.5, timeout: Optional[float] = None) -> "Run":
        deadline = None if timeout is None else time.monotonic() + timeout
        while self.read()["state"] == "running":
            if deadline is not None and time.monotonic() > deadline:
                raise TimeoutError(f"run {self.id} still running")
            time.sleep(interval)
        return self

    def write(self, data: str, close: bool = False) -> None:
        self.machine._client.call("POST", f"{self._path}/stdin", {"data": data, "close": close})

    def signal(self, signal: Any = "TERM") -> None:
        self.machine._client.call("POST", f"{self._path}/signal", {"signal": signal})


class Runs:
    def __init__(self, machine: Machine):
        self._machine = machine

    def start(
        self,
        cmd: str,
        cwd: Optional[str] = None,
        env: Optional[Mapping[str, str]] = None,
        user: Optional[str] = None,
        stdin: Optional[str] = None,
    ) -> Run:
        body: Dict[str, Any] = {"cmd": cmd}
        for key, value in [("cwd", cwd), ("env", dict(env) if env else None), ("user", user), ("stdin", stdin)]:
            if value is not None:
                body[key] = value
        return Run(self._machine, self._machine._client.call("POST", f"{self._machine._path}/runs", body))

    def list(self) -> List[Run]:
        return [Run(self._machine, r) for r in self._machine._client.call("GET", f"{self._machine._path}/runs")]

    def get(self, run_id: int) -> Run:
        run = Run(self._machine, {"runID": run_id})
        run.read()
        return run


class Prompt:
    """A coding agent run in a machine on a prompt."""

    def __init__(self, machine: Machine, described: Mapping[str, Any]):
        self.machine = machine
        self.info: Dict[str, Any] = dict(described)
        self.id: int = described["promptID"]

    @property
    def _path(self) -> str:
        return f"{self.machine._path}/prompts/{self.id}"

    def read(self, after: int = 0) -> Dict[str, Any]:
        answer = self.machine._client.call("GET", self._path, query={"after": after})
        self.info = answer
        return answer

    def events(self, interval: float = 0.5) -> Iterator[Dict[str, Any]]:
        """Each event as the agent prints it, until it ends: `{"event": ...}`
        for a line of JSON, `{"text": ...}` for any other."""
        after = 0
        while True:
            answer = self.read(after)
            yield from answer["events"]
            after = answer["next"]
            if answer["state"] != "running":
                return
            time.sleep(interval)

    def interrupt(self) -> None:
        self.machine._client.call("POST", f"{self._path}/interrupt")


class Prompts:
    def __init__(self, machine: Machine):
        self._machine = machine

    def start(
        self,
        provider: str,
        prompt: str,
        model: Optional[str] = None,
        resume: Optional[str] = None,
        unattended: bool = False,
        command: Optional[str] = None,
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Mapping[str, str]] = None,
    ) -> Prompt:
        body: Dict[str, Any] = {"provider": provider, "prompt": prompt}
        for key, value in [
            ("model", model),
            ("resume", resume),
            ("command", command),
            ("cwd", cwd),
            ("user", user),
            ("env", dict(env) if env else None),
        ]:
            if value is not None:
                body[key] = value
        if unattended:
            body["unattended"] = True
        return Prompt(self._machine, self._machine._client.call("POST", f"{self._machine._path}/prompts", body))

    def list(self) -> List[Prompt]:
        return [Prompt(self._machine, p) for p in self._machine._client.call("GET", f"{self._machine._path}/prompts")]


class Files:
    def __init__(self, machine: Machine):
        self._machine = machine

    def write(self, path: str, data: bytes, user: Optional[str] = None) -> None:
        """Put `data` at `path` in the guest, making the directories above it."""
        if isinstance(data, str):
            data = data.encode()
        with self._machine._client.raw(
            "PUT",
            f"{self._machine._path}/files",
            data,
            {"path": path, "user": user},
            "application/octet-stream",
        ):
            pass

    def read(self, path: str) -> bytes:
        with self._machine._client.raw("GET", f"{self._machine._path}/files", query={"path": path}) as response:
            return response.read()


class Ports:
    def __init__(self, machine: Machine):
        self._machine = machine

    def publish(self, guest_port: int, host_port: Optional[int] = None, proxy: bool = False) -> Dict[str, Any]:
        body: Dict[str, Any] = {"guestPort": guest_port}
        if host_port is not None:
            body["hostPort"] = host_port
        if proxy:
            body["proxy"] = True
        return self._machine._client.call("POST", f"{self._machine._path}/ports", body)

    def list(self) -> List[Dict[str, Any]]:
        return self._machine._client.call("GET", f"{self._machine._path}/ports")

    def unpublish(self, host_port: int) -> None:
        self._machine._client.call("DELETE", f"{self._machine._path}/ports/{int(host_port)}")


class Snapshot:
    """A copy of a stopped machine's disk."""

    def __init__(self, machine: Machine, described: Mapping[str, Any]):
        self.machine = machine
        self.info: Dict[str, Any] = dict(described)
        self.id: str = described["snapshotID"]
        self.name: str = described.get("name", "")

    def __repr__(self) -> str:
        return f"Snapshot({self.id!r}, {self.name!r})"

    @property
    def _path(self) -> str:
        return f"{self.machine._path}/snapshots/{_seg(self.id)}"

    def restore(self) -> Machine:
        self.machine._set(self.machine._client.call("POST", f"{self._path}/restore", {}, timeout=3600))
        return self.machine

    def delete(self) -> None:
        self.machine._client.call("DELETE", self._path)

    def listdir(self, path: str = "/", partition: Optional[int] = None) -> List[Dict[str, Any]]:
        """A directory of its disk, read without booting it."""
        answer = self.machine._client.call(
            "GET", f"{self._path}/files", query={"path": path, "partition": partition}
        )
        return answer["entries"]

    def read(self, path: str, partition: Optional[int] = None) -> bytes:
        """A file of its disk, read without booting it."""
        with self.machine._client.raw(
            "GET", f"{self._path}/file", query={"path": path, "partition": partition}
        ) as response:
            return response.read()

    def download(self, destination: str, chunk: int = 1 << 20) -> int:
        """Its whole disk, raw, into `destination`, which must not exist;
        runs of zeros are left as holes. The bytes written."""
        written = 0
        with self.machine._client.raw("GET", f"{self._path}/disk", timeout=3600) as response:
            with open(destination, "xb") as out:
                while True:
                    piece = response.read(chunk)
                    if not piece:
                        break
                    if piece.count(0) == len(piece):
                        out.seek(len(piece), os.SEEK_CUR)
                    else:
                        out.write(piece)
                    written += len(piece)
                out.truncate(written)
        return written


class Snapshots:
    def __init__(self, machine: Machine):
        self._machine = machine

    def take(self, name: Optional[str] = None) -> Snapshot:
        body = {} if name is None else {"name": name}
        return Snapshot(
            self._machine,
            self._machine._client.call("POST", f"{self._machine._path}/snapshots", body, timeout=3600),
        )

    def list(self) -> List[Snapshot]:
        return [
            Snapshot(self._machine, s)
            for s in self._machine._client.call("GET", f"{self._machine._path}/snapshots")
        ]

    def get(self, snapshot_id: str) -> Snapshot:
        return Snapshot(
            self._machine,
            self._machine._client.call("GET", f"{self._machine._path}/snapshots/{_seg(snapshot_id)}"),
        )
