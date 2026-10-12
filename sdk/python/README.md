# HyperMachine for Python

A client for HyperMachine's [machine](../../docs/MACHINES.md) API, with no
dependencies beyond Python 3.9's standard library.

```python
from hypermachine import Client

hm = Client("http://node:3980", api_key="...")   # or HV2_SANDBOX_URL / HV2_API_KEY

web = hm.machines.create("web-01", cpu_count=2, memory_mb=2048, disk_gib=20)
print(web.exec("uname -a").stdout)

run = web.runs.start("make test", cwd="/srv/app", env={"CI": "1"})
for chunk in run.follow():
    print(chunk, end="")
print(run.exit_code)

web.files.write("/etc/app/config.toml", b"port = 8080\n")
web.ports.publish(8080, proxy=True)

web.stop()
web.resize(cpu_count=4, disk_gib=40)
snap = web.snapshots.take("before upgrade")
print(snap.read("/etc/hostname"))          # read without booting it
snap.restore()
web.start()

prompt = web.prompts.start("claude", "make the tests pass", user="dev",
                           unattended=True, env={"ANTHROPIC_API_KEY": "..."})
for event in prompt.events():
    print(event)

print(hm.usage(start="2026-10-01T00:00:00Z", by="template")["total"])
hm.put_environment("web", {"API_URL": "https://api.example"}, {"/etc/app/ca.pem": ca})
```

| | |
|---|---|
| `client.machines` | `create`, `list`, `get` |
| `Machine` | `start`, `stop`, `restart`, `delete`, `resize`, `refresh`, `exec` |
| `machine.runs` | `start`, `list`, `get`; a `Run` has `read`, `follow`, `wait`, `write`, `signal` |
| `machine.prompts` | `start`, `list`; a `Prompt` has `read`, `events`, `interrupt` |
| `machine.files` | `write`, `read` |
| `machine.ports` | `publish`, `list`, `unpublish` |
| `machine.snapshots` | `take`, `list`, `get`; a `Snapshot` has `restore`, `delete`, `listdir`, `read`, `download` |
| `client` | `usage`, `put_environment`, `environments`, `delete_environment`, and `call` for any other route |

- **One call is one request**, to a node or a control plane, which answer
  the same routes. Nothing is cached: an object's `info` is what the node
  said when it was last fetched.
- **A refusal raises `ApiError`**, with `status` and the node's own
  `message`.
- **Names are one path segment each**, quoted, so no name reaches another
  route.
- **The key goes in `x-api-key`** and nowhere else; `repr` does not show it.
- `Snapshot.download` writes runs of zeros as holes and never over a file
  that is there.

It is not on PyPI. Use it from a checkout:

```sh
pip install /path/to/HyperMachine/sdk/python
```

## Tested

- Against an HTTP fixture that answers as a node does, on every CI run:
  `python3 -m unittest discover -s sdk/python/tests`.
- Against a real node on KVM, every part above but prompts:
  [the evidence](../../docs/benchmarks/2026-10-12/python-sdk-kvm/README.md).

## Not here

Sandboxes, which E2B's own SDKs drive through HyperMachine's E2B API; an
async client; a TypeScript one.
