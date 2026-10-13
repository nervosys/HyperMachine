// Types for the client in index.js. The routes are documented in
// docs/MACHINES.md, docs/USAGE.md and docs/ENVIRONMENTS.md.

/** The node refused, or could not do, what was asked. */
export class ApiError extends Error {
  readonly status: number;
  /** The node's own words. */
  readonly detail: string;
}

export interface ClientOptions {
  /** HV2_SANDBOX_URL, then http://127.0.0.1:3980, unless given. */
  endpoint?: string;
  /** HV2_API_KEY unless given. Sent as `x-api-key` only. */
  apiKey?: string;
  timeoutMs?: number;
}

export interface ExecResult {
  exitCode: number | null;
  signal: number | null;
  stdout: string;
  stderr: string;
}

export interface UsageOptions {
  /** RFC 3339 text, or epoch milliseconds. */
  from?: string | number;
  to?: string | number;
  by?: 'sandbox' | 'team' | 'template' | 'owner';
}

export class Client {
  constructor(options?: ClientOptions);
  readonly endpoint: string;
  readonly machines: Machines;
  /** One request; the Response. Throws ApiError on a refusal. */
  raw(
    method: string,
    path: string,
    options?: { body?: BodyInit; params?: Record<string, unknown>; contentType?: string; timeoutMs?: number },
  ): Promise<Response>;
  /** One JSON request to any route; the JSON answer, or null. */
  call<T = any>(method: string, path: string, body?: unknown, options?: { params?: Record<string, unknown>; timeoutMs?: number }): Promise<T>;
  usage(options?: UsageOptions): Promise<any>;
  putEnvironment(
    name: string,
    environment?: { envVars?: Record<string, string>; files?: Record<string, Uint8Array | string> },
  ): Promise<any>;
  environments(): Promise<any[]>;
  deleteEnvironment(name: string): Promise<void>;
}

export interface CreateOptions {
  template?: string;
  image?: string;
  cpuCount?: number;
  memoryMB?: number;
  diskGiB?: number;
  start?: boolean;
  guestAgent?: boolean;
  userData?: string;
  network?: Record<string, unknown>;
  /** Coding agents to install at first boot, for prompts. Needs `guestAgent`. */
  agents?: Array<'claude' | 'codex'>;
  /** A desktop, and computer use on it: `true` for 1280x800, or a size. Needs `guestAgent`. */
  desktop?: true | { width: number; height: number };
  [field: string]: unknown;
}

export interface Machines {
  create(name: string, options?: CreateOptions): Promise<Machine>;
  list(): Promise<Machine[]>;
  get(name: string): Promise<Machine>;
}

export class Machine {
  readonly name: string;
  readonly id: string;
  readonly state: string;
  /** What the node said when this was last fetched. */
  readonly info: Record<string, any>;
  readonly runs: Runs;
  readonly prompts: Prompts;
  readonly conversations: Conversations;
  readonly desktop: Desktop;
  readonly files: Files;
  readonly ports: Ports;
  readonly snapshots: Snapshots;
  refresh(): Promise<Machine>;
  start(): Promise<Machine>;
  stop(): Promise<Machine>;
  restart(): Promise<Machine>;
  delete(): Promise<void>;
  resize(size: { cpuCount?: number; memoryMB?: number; diskGiB?: number }): Promise<Machine>;
  exec(cmd: string, options?: { timeoutSecs?: number }): Promise<ExecResult>;
  processes(options?: { all?: boolean }): Promise<GuestProcess[]>;
}

/** One process in a machine's guest, as `ps` lists it. */
export interface GuestProcess {
  pid: number;
  ppid: number;
  uid: number | null;
  state: string;
  command: string;
  args: string;
  threads: number;
  rssKiB: number;
  cpuSeconds: number;
  startedSecondsAgo: number;
}

export class Run {
  readonly id: number;
  readonly info: Record<string, any>;
  readonly state: string;
  readonly exitCode: number | null;
  read(offsets?: { stdout?: number; stderr?: number }): Promise<Record<string, any>>;
  follow(options?: { intervalMs?: number }): AsyncGenerator<string>;
  wait(options?: { intervalMs?: number; timeoutMs?: number }): Promise<Run>;
  write(data: string, options?: { close?: boolean }): Promise<void>;
  signal(signal?: string | number): Promise<void>;
}

export interface Runs {
  start(cmd: string, options?: { cwd?: string; env?: Record<string, string>; user?: string; stdin?: string }): Promise<Run>;
  list(): Promise<Run[]>;
  get(runID: number): Promise<Run>;
}

export interface PromptEvent {
  offset: number;
  /** A line of the agent's JSON output. */
  event?: Record<string, any>;
  /** A line that was not JSON. */
  text?: string;
}

export class Prompt {
  readonly id: number;
  readonly info: Record<string, any>;
  read(after?: number): Promise<Record<string, any>>;
  events(options?: { intervalMs?: number }): AsyncGenerator<PromptEvent>;
  interrupt(): Promise<void>;
}

export interface Prompts {
  start(
    provider: 'claude' | 'codex' | 'custom',
    prompt: string,
    options?: {
      model?: string;
      resume?: string;
      unattended?: boolean;
      command?: string;
      cwd?: string;
      user?: string;
      env?: Record<string, string>;
    },
  ): Promise<Prompt>;
  list(): Promise<Prompt[]>;
}

export type MouseButton = 'left' | 'middle' | 'right';

export interface Desktop {
  info(): Promise<{ width: number; height: number; ready: boolean; webPort: number; webPath: string }>;
  screenshot(): Promise<Buffer>;
  act(
    action: 'move' | 'click' | 'drag' | 'type' | 'key' | 'scroll' | 'cursor',
    fields?: Record<string, unknown>,
  ): Promise<[number, number]>;
  click(x: number, y: number, options?: { button?: MouseButton; count?: 1 | 2 | 3 }): Promise<[number, number]>;
  move(x: number, y: number): Promise<[number, number]>;
  drag(x: number, y: number, toX: number, toY: number, options?: { button?: MouseButton }): Promise<[number, number]>;
  type(text: string): Promise<[number, number]>;
  key(keys: string): Promise<[number, number]>;
  scroll(
    x: number,
    y: number,
    options?: { direction?: 'up' | 'down' | 'left' | 'right'; amount?: number },
  ): Promise<[number, number]>;
}

export interface TranscriptLine {
  role: 'user' | 'assistant';
  text: string;
  promptID: number;
}

export class Conversation {
  readonly id: number;
  readonly name: string | null;
  readonly info: Record<string, any>;
  /** The turn the message went to; `info.steered` says whether it was running. */
  say(prompt: string, options?: { env?: Record<string, string> }): Promise<Prompt>;
  refresh(): Promise<Conversation>;
  transcript(): Promise<TranscriptLine[]>;
  interrupt(): Promise<void>;
  delete(): Promise<void>;
}

export interface Conversations {
  create(
    provider: 'claude' | 'codex',
    options?: {
      name?: string;
      model?: string;
      user?: string;
      cwd?: string;
      unattended?: boolean;
      from?: number | string;
    },
  ): Promise<Conversation>;
  get(key: number | string): Promise<Conversation>;
  list(): Promise<Conversation[]>;
}

export interface Files {
  write(path: string, data: Uint8Array | string, options?: { user?: string }): Promise<void>;
  read(path: string): Promise<Buffer>;
}

export interface Ports {
  publish(guestPort: number, options?: { hostPort?: number; proxy?: boolean }): Promise<any>;
  list(): Promise<any[]>;
  unpublish(hostPort: number): Promise<void>;
}

export interface DirectoryEntry {
  name: string;
  type: 'file' | 'directory' | 'symlink' | 'other';
  mode: string;
  uid: number;
  gid: number;
  size: number | null;
}

export class Snapshot {
  readonly id: string;
  readonly name: string;
  readonly info: Record<string, any>;
  restore(): Promise<Machine>;
  delete(): Promise<void>;
  listdir(path?: string, options?: { partition?: number }): Promise<DirectoryEntry[]>;
  read(path: string, options?: { partition?: number }): Promise<Buffer>;
  /** Into `destination`, which must not exist; zeros left as holes. */
  download(destination: string): Promise<number>;
}

export interface Snapshots {
  take(name?: string): Promise<Snapshot>;
  list(): Promise<Snapshot[]>;
  get(snapshotID: string): Promise<Snapshot>;
}
