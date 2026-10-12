# HyperMachine for TypeScript and JavaScript

A client for HyperMachine's [machine](../../docs/MACHINES.md) API, for Node 18+
and any runtime with `fetch`. No dependencies; types in `index.d.ts`.

```ts
import { ApiError, Client } from '@nervosys/hypermachine';

const hm = new Client({ endpoint: 'http://node:3980', apiKey: '...' });  // or HV2_SANDBOX_URL / HV2_API_KEY

const web = await hm.machines.create('web-01', { cpuCount: 2, memoryMB: 2048, diskGiB: 20 });
console.log((await web.exec('uname -a')).stdout);

const run = await web.runs.start('make test', { cwd: '/srv/app', env: { CI: '1' } });
for await (const chunk of run.follow()) process.stdout.write(chunk);
console.log(run.exitCode);

await web.files.write('/etc/app/config.toml', 'port = 8080\n');
await web.ports.publish(8080, { proxy: true });

await web.stop();
await web.resize({ cpuCount: 4, diskGiB: 40 });
const snap = await web.snapshots.take('before upgrade');
console.log((await snap.read('/etc/hostname')).toString());   // read without booting it
await snap.restore();
await web.start();

const prompt = await web.prompts.start('claude', 'make the tests pass',
  { user: 'dev', unattended: true, env: { ANTHROPIC_API_KEY: '...' } });
for await (const event of prompt.events()) console.log(event);

console.log((await hm.usage({ from: '2026-10-01T00:00:00Z', by: 'template' })).total);
await hm.putEnvironment('web', { envVars: { API_URL: 'https://api.example' } });
```

It has what [the Python client](../python/README.md) has, under the same
names in camelCase: machines, `exec`, runs, prompts, files, ports, snapshots,
usage and environments, and `call` for any other route.

- **One call is one request.** Nothing is cached; `info` is what the node
  said last.
- **A refusal throws `ApiError`**, with `status` and the node's own words in
  `detail`.
- **A name is one path segment**, encoded, so no name reaches another route.
- **The key goes in `x-api-key`** and nowhere else; it is not one of the
  client's own keys, so it is not printed or serialized with it.
- `Snapshot.download` leaves runs of zeros as holes and never writes over a
  file that is there.

It is not on npm. Use it from a checkout:

```sh
npm install /path/to/HyperMachine/sdk/typescript
```

## Tested

- Against an HTTP fixture that answers as a node does, and its types with
  `tsc --strict`, including calls they must refuse, on every CI run.
- Against a real node on KVM, every part above but prompts:
  [the evidence](../../docs/benchmarks/2026-10-12/typescript-sdk-kvm/README.md).

## Not here

Sandboxes, which E2B's own SDK drives through HyperMachine's E2B API; a
browser build, since a key does not belong in a page.
