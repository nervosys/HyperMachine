// A client for HyperMachine's machine API, for Node 18+ and other runtimes
// with `fetch`. No dependencies. Types are in index.d.ts.
//
// Every call is one HTTP request to a node or a control plane, which answer
// the same routes. A refusal throws ApiError with the status and the node's
// own words. Nothing is cached: an object's `info` is what the node said
// when it was last fetched.

/** The node refused, or could not do, what was asked. */
export class ApiError extends Error {
  constructor(status, message, method, path) {
    super(`${method} ${path}: ${status} ${message}`);
    this.name = 'ApiError';
    this.status = status;
    this.detail = message;
  }
}

/** One path segment, so a name can never step into another route. */
const seg = (part) => encodeURIComponent(String(part));

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function query(pairs) {
  const kept = Object.entries(pairs ?? {}).filter(([, v]) => v !== undefined && v !== null);
  return kept.length ? '?' + new URLSearchParams(kept.map(([k, v]) => [k, String(v)])).toString() : '';
}

/** Only the keys whose values were given. */
function given(pairs) {
  return Object.fromEntries(Object.entries(pairs).filter(([, v]) => v !== undefined && v !== null));
}

export class Client {
  /**
   * `endpoint` defaults to HV2_SANDBOX_URL, then http://127.0.0.1:3980;
   * `apiKey` to HV2_API_KEY. The key is sent as `x-api-key` and nowhere else.
   */
  constructor({ endpoint, apiKey, timeoutMs = 120_000 } = {}) {
    const env = typeof process !== 'undefined' ? process.env : {};
    this.endpoint = (endpoint ?? env.HV2_SANDBOX_URL ?? 'http://127.0.0.1:3980').replace(/\/+$/, '');
    // Not enumerable: not printed, not serialized.
    Object.defineProperty(this, 'apiKey', { value: apiKey ?? env.HV2_API_KEY, enumerable: false });
    this.timeoutMs = timeoutMs;
    this.machines = new Machines(this);
  }

  /** One request; the Response. Throws ApiError on a refusal. */
  async raw(method, path, { body, params, contentType, timeoutMs } = {}) {
    const headers = {};
    if (contentType) headers['content-type'] = contentType;
    if (this.apiKey) headers['x-api-key'] = this.apiKey;
    const response = await fetch(this.endpoint + path + query(params), {
      method,
      headers,
      body,
      signal: AbortSignal.timeout(timeoutMs ?? this.timeoutMs),
    });
    if (!response.ok) {
      const text = await response.text();
      let message = text;
      try {
        message = JSON.parse(text).message ?? text;
      } catch {
        // Not JSON: the text is the message.
      }
      throw new ApiError(response.status, message, method, path);
    }
    return response;
  }

  /** One JSON request; the JSON answer, or null for an empty one. */
  async call(method, path, body, { params, timeoutMs } = {}) {
    const response = await this.raw(method, path, {
      body: body === undefined ? undefined : JSON.stringify(body),
      params,
      contentType: body === undefined ? undefined : 'application/json',
      timeoutMs,
    });
    const text = await response.text();
    return text.trim() ? JSON.parse(text) : null;
  }

  /** Running time, vCPU-seconds and GiB-seconds in a window. See docs/USAGE.md. */
  usage({ from, to, by = 'sandbox' } = {}) {
    return this.call('GET', '/usage', undefined, { params: { from, to, by } });
  }

  /** Make or replace a team's environment: variables, and files by path. */
  putEnvironment(name, { envVars = {}, files = {} } = {}) {
    return this.call('PUT', `/environments/${seg(name)}`, {
      envVars,
      files: Object.entries(files).map(([path, data]) => ({
        path,
        contentBase64: Buffer.from(data).toString('base64'),
      })),
    });
  }

  environments() {
    return this.call('GET', '/environments');
  }

  async deleteEnvironment(name) {
    await this.call('DELETE', `/environments/${seg(name)}`);
  }

  toString() {
    return `Client(${this.endpoint})`;
  }
}

class Machines {
  constructor(client) {
    this.client = client;
  }

  async create(name, { template, image, cpuCount, memoryMB, diskGiB, start = true, guestAgent, userData, network, agents, desktop, ...more } = {}) {
    const body = {
      name,
      start,
      ...given({ templateID: template, image, cpuCount, memoryMB, diskGiB, guestAgent, userData, network, agents, desktop }),
      ...more,
    };
    return new Machine(this.client, await this.client.call('POST', '/machines', body));
  }

  async list() {
    return (await this.client.call('GET', '/machines')).map((m) => new Machine(this.client, m));
  }

  async get(name) {
    return new Machine(this.client, await this.client.call('GET', `/machines/${seg(name)}`));
  }
}

export class Machine {
  constructor(client, described) {
    this.client = client;
    this.set(described);
    this.runs = new Runs(this);
    this.prompts = new Prompts(this);
    this.conversations = new Conversations(this);
    this.desktop = new Desktop(this);
    this.files = new Files(this);
    this.ports = new Ports(this);
    this.snapshots = new Snapshots(this);
  }

  set(described) {
    this.info = { ...described };
    this.name = described.name;
    this.id = described.machineID ?? '';
    this.state = described.state ?? '';
  }

  get path() {
    return `/machines/${seg(this.name)}`;
  }

  async refresh() {
    this.set(await this.client.call('GET', this.path));
    return this;
  }

  async #act(action) {
    this.set(await this.client.call('POST', `${this.path}/${action}`, undefined, { timeoutMs: 600_000 }));
    return this;
  }

  start() {
    return this.#act('start');
  }

  stop() {
    return this.#act('stop');
  }

  restart() {
    return this.#act('restart');
  }

  async delete() {
    await this.client.call('DELETE', this.path);
  }

  /** Another size for a stopped machine, there at its next start. */
  async resize({ cpuCount, memoryMB, diskGiB } = {}) {
    this.set(await this.client.call('PATCH', this.path, given({ cpuCount, memoryMB, diskGiB })));
    return this;
  }

  /** Every process in the guest, as `ps` lists them; kernel threads only with `all`. */
  async processes({ all = false } = {}) {
    const answer = await this.client.call('GET', `${this.path}/processes${all ? '?all=true' : ''}`);
    return answer.processes;
  }

  /** Run a shell line in the guest and wait for it. */
  async exec(cmd, { timeoutSecs = 60 } = {}) {
    const answer = await this.client.call(
      'POST',
      `${this.path}/exec`,
      { cmd, timeout_secs: timeoutSecs },
      { timeoutMs: (timeoutSecs + 30) * 1000 },
    );
    return { exitCode: answer.exit_code ?? null, signal: answer.signal ?? null, stdout: answer.stdout ?? '', stderr: answer.stderr ?? '' };
  }
}

export class Run {
  constructor(machine, described) {
    this.machine = machine;
    this.info = { ...described };
    this.id = described.runID;
  }

  get path() {
    return `${this.machine.path}/runs/${this.id}`;
  }

  get state() {
    return this.info.state;
  }

  get exitCode() {
    return this.info.exitCode ?? null;
  }

  /** The run, and what it printed from these byte offsets on. */
  async read({ stdout = 0, stderr = 0 } = {}) {
    this.info = await this.machine.client.call('GET', this.path, undefined, { params: { stdout, stderr } });
    return this.info;
  }

  /** Its standard output as it arrives, until it ends. */
  async *follow({ intervalMs = 500 } = {}) {
    let offset = 0;
    for (;;) {
      const answer = await this.read({ stdout: offset });
      if (answer.stdout.text) yield answer.stdout.text;
      offset = answer.stdout.next;
      if (answer.state !== 'running') return;
      await sleep(intervalMs);
    }
  }

  async wait({ intervalMs = 500, timeoutMs } = {}) {
    const deadline = timeoutMs === undefined ? Infinity : Date.now() + timeoutMs;
    while ((await this.read()).state === 'running') {
      if (Date.now() > deadline) throw new Error(`run ${this.id} still running`);
      await sleep(intervalMs);
    }
    return this;
  }

  async write(data, { close = false } = {}) {
    await this.machine.client.call('POST', `${this.path}/stdin`, { data, close });
  }

  async signal(signal = 'TERM') {
    await this.machine.client.call('POST', `${this.path}/signal`, { signal });
  }
}

class Runs {
  constructor(machine) {
    this.machine = machine;
  }

  async start(cmd, { cwd, env, user, stdin } = {}) {
    return new Run(this.machine, await this.machine.client.call('POST', `${this.machine.path}/runs`, { cmd, ...given({ cwd, env, user, stdin }) }));
  }

  async list() {
    return (await this.machine.client.call('GET', `${this.machine.path}/runs`)).map((r) => new Run(this.machine, r));
  }

  async get(runID) {
    const run = new Run(this.machine, { runID });
    await run.read();
    return run;
  }
}

export class Prompt {
  constructor(machine, described) {
    this.machine = machine;
    this.info = { ...described };
    this.id = described.promptID;
  }

  get path() {
    return `${this.machine.path}/prompts/${this.id}`;
  }

  async read(after = 0) {
    this.info = await this.machine.client.call('GET', this.path, undefined, { params: { after } });
    return this.info;
  }

  /** Each event as the agent prints it, until it ends. */
  async *events({ intervalMs = 500 } = {}) {
    let after = 0;
    for (;;) {
      const answer = await this.read(after);
      yield* answer.events;
      after = answer.next;
      if (answer.state !== 'running') return;
      await sleep(intervalMs);
    }
  }

  async interrupt() {
    await this.machine.client.call('POST', `${this.path}/interrupt`);
  }
}

class Prompts {
  constructor(machine) {
    this.machine = machine;
  }

  async start(provider, prompt, { model, resume, unattended, command, cwd, user, env } = {}) {
    const body = { provider, prompt, ...given({ model, resume, command, cwd, user, env }) };
    if (unattended) body.unattended = true;
    return new Prompt(this.machine, await this.machine.client.call('POST', `${this.machine.path}/prompts`, body));
  }

  async list() {
    return (await this.machine.client.call('GET', `${this.machine.path}/prompts`)).map((p) => new Prompt(this.machine, p));
  }
}

/** A coding agent talked to over many turns, each a prompt. */
export class Conversation {
  constructor(machine, described) {
    this.machine = machine;
    this.info = { ...described };
    this.id = described.conversationID;
    this.name = described.name ?? null;
  }

  get path() {
    return `${this.machine.path}/conversations/${this.id}`;
  }

  /**
   * Send a message. With nothing running it starts a turn; with a turn of
   * Claude Code's running it goes into that turn, and `info.steered` is true.
   * Either way, the prompt returned is the turn.
   */
  async say(prompt, { env } = {}) {
    const body = { prompt, ...given({ env }) };
    return new Prompt(this.machine, await this.machine.client.call('POST', `${this.path}/messages`, body));
  }

  async refresh() {
    this.info = await this.machine.client.call('GET', this.path);
    return this;
  }

  /** Who said what, in order: `{ role, text, promptID }`. */
  async transcript() {
    return this.machine.client.call('GET', `${this.path}/transcript`);
  }

  async interrupt() {
    await this.machine.client.call('POST', `${this.path}/interrupt`);
  }

  async delete() {
    await this.machine.client.call('DELETE', this.path);
  }
}

class Conversations {
  constructor(machine) {
    this.machine = machine;
  }

  /** `from` is another of this machine's, by number or name, to start from. */
  async create(provider, { name, model, user, cwd, unattended, from } = {}) {
    const body = { provider, ...given({ name, model, user, cwd }) };
    if (from !== undefined && from !== null) body.from = String(from);
    if (unattended) body.unattended = true;
    return new Conversation(this.machine, await this.machine.client.call('POST', `${this.machine.path}/conversations`, body));
  }

  async get(key) {
    return new Conversation(this.machine, await this.machine.client.call('GET', `${this.machine.path}/conversations/${seg(key)}`));
  }

  async list() {
    return (await this.machine.client.call('GET', `${this.machine.path}/conversations`)).map((c) => new Conversation(this.machine, c));
  }
}

/** A machine's desktop, and computer use on it: for a machine created with `desktop`. */
class Desktop {
  constructor(machine) {
    this.machine = machine;
  }

  get path() {
    return `${this.machine.path}/desktop`;
  }

  /** Its size, whether it is up, and the guest port noVNC serves it on. */
  async info() {
    return this.machine.client.call('GET', this.path);
  }

  /** A PNG of the whole screen. */
  async screenshot() {
    const response = await this.machine.client.raw('GET', `${this.path}/screenshot`);
    return Buffer.from(await response.arrayBuffer());
  }

  /** One action; answers with where the pointer is, `[x, y]`. */
  async act(action, fields = {}) {
    return (await this.machine.client.call('POST', `${this.path}/actions`, { action, ...fields })).cursor;
  }

  click(x, y, { button = 'left', count = 1 } = {}) {
    return this.act('click', { x, y, button, count });
  }

  move(x, y) {
    return this.act('move', { x, y });
  }

  drag(x, y, toX, toY, { button = 'left' } = {}) {
    return this.act('drag', { x, y, toX, toY, button });
  }

  type(text) {
    return this.act('type', { text });
  }

  /** X key names or chords, such as `Return` or `ctrl+c`, separated by spaces. */
  key(keys) {
    return this.act('key', { keys });
  }

  scroll(x, y, { direction = 'down', amount = 3 } = {}) {
    return this.act('scroll', { x, y, direction, amount });
  }
}

class Files {
  constructor(machine) {
    this.machine = machine;
  }

  /** Put `data` at `path` in the guest, making the directories above it. */
  async write(path, data, { user } = {}) {
    const body = typeof data === 'string' ? Buffer.from(data) : data;
    await this.machine.client.raw('PUT', `${this.machine.path}/files`, {
      body,
      params: { path, user },
      contentType: 'application/octet-stream',
    });
  }

  async read(path) {
    const response = await this.machine.client.raw('GET', `${this.machine.path}/files`, { params: { path } });
    return Buffer.from(await response.arrayBuffer());
  }
}

class Ports {
  constructor(machine) {
    this.machine = machine;
  }

  publish(guestPort, { hostPort, proxy } = {}) {
    const body = { guestPort, ...given({ hostPort }) };
    if (proxy) body.proxy = true;
    return this.machine.client.call('POST', `${this.machine.path}/ports`, body);
  }

  list() {
    return this.machine.client.call('GET', `${this.machine.path}/ports`);
  }

  async unpublish(hostPort) {
    await this.machine.client.call('DELETE', `${this.machine.path}/ports/${Number(hostPort)}`);
  }
}

export class Snapshot {
  constructor(machine, described) {
    this.machine = machine;
    this.info = { ...described };
    this.id = described.snapshotID;
    this.name = described.name ?? '';
  }

  get path() {
    return `${this.machine.path}/snapshots/${seg(this.id)}`;
  }

  async restore() {
    this.machine.set(await this.machine.client.call('POST', `${this.path}/restore`, {}, { timeoutMs: 3_600_000 }));
    return this.machine;
  }

  async delete() {
    await this.machine.client.call('DELETE', this.path);
  }

  /** A directory of its disk, read without booting it. */
  async listdir(path = '/', { partition } = {}) {
    return (await this.machine.client.call('GET', `${this.path}/files`, undefined, { params: { path, partition } })).entries;
  }

  /** A file of its disk, read without booting it. */
  async read(path, { partition } = {}) {
    const response = await this.machine.client.raw('GET', `${this.path}/file`, { params: { path, partition } });
    return Buffer.from(await response.arrayBuffer());
  }

  /**
   * Its whole disk, raw, into `destination`, which must not exist; runs of
   * zeros are left as holes. Resolves to the bytes written.
   */
  async download(destination) {
    const { open } = await import('node:fs/promises');
    const response = await this.machine.client.raw('GET', `${this.path}/disk`, { timeoutMs: 3_600_000 });
    const file = await open(destination, 'wx');
    let written = 0;
    try {
      for await (const chunk of response.body) {
        if (!chunk.some((byte) => byte !== 0)) {
          written += chunk.length;
          continue;
        }
        await file.write(chunk, 0, chunk.length, written);
        written += chunk.length;
      }
      await file.truncate(written);
    } finally {
      await file.close();
    }
    return written;
  }
}

class Snapshots {
  constructor(machine) {
    this.machine = machine;
  }

  async take(name) {
    const body = name === undefined ? {} : { name };
    return new Snapshot(this.machine, await this.machine.client.call('POST', `${this.machine.path}/snapshots`, body, { timeoutMs: 3_600_000 }));
  }

  async list() {
    return (await this.machine.client.call('GET', `${this.machine.path}/snapshots`)).map((s) => new Snapshot(this.machine, s));
  }

  async get(snapshotID) {
    return new Snapshot(this.machine, await this.machine.client.call('GET', `${this.machine.path}/snapshots/${seg(snapshotID)}`));
  }
}
