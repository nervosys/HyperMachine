// Types for the request and response of `hm sandbox exec`, version 1.
// The format's own definition is docs/schemas/sandbox-request-v1.schema.json.

/** The request format this client writes. */
export const VERSION: 1;

/** One sandboxed run: a command and what contains it. */
export interface Request {
  /** Filled in when absent. */
  version?: 1;
  /** The program, then its arguments. Run directly, never through a shell. */
  command: [string, ...string[]];
  /** The workload's whole environment. Nothing is inherited. */
  env?: Record<string, string>;
  /** Where it starts. Absent, the backend chooses. */
  workingDir?: string | null;
  /** Text written to its standard input. */
  stdin?: string | null;
  /** Ceilings on what it consumes. Each is unlimited when absent. */
  limits?: {
    memoryBytes?: number | null;
    maxProcesses?: number | null;
    cpuTimeMs?: number | null;
    /** Wall-clock time before it is killed. */
    timeoutMs?: number | null;
  };
  network?: {
    /** `deny` (the default): no network at all. `host`: the host's, unrestricted. */
    egress?: 'deny' | 'host';
    /**
     * Hosts it may reach and no others: `example.com`, `*.example.com`, an
     * address or a CIDR range. Only with `egress` left at `deny`. The
     * workload is given a proxy and `HTTPS_PROXY` and its like pointing at
     * it; a program that ignores them reaches nothing. Linux and macOS.
     */
    allow?: string[];
  };
  filesystem?: {
    /** A directory that becomes its root, hiding the host's. */
    root?: string | null;
    /** Absolute, existing paths it may read. */
    readOnly?: string[];
    /** Absolute, existing paths it may read and write. */
    readWrite?: string[];
    /** Absolute, existing paths closed to it, whatever else would let it in. */
    denied?: string[];
    /** Whether readOnly and readWrite are all of the caller's filesystem it reaches. */
    confine?: boolean;
  };
  /** Hide the host's processes from it. */
  isolateProcesses?: boolean;
  /** Bar it from gaining privileges. */
  noNewPrivileges?: boolean;
  /** Keep it from the desktop it was started on. Windows only. */
  isolateUi?: boolean;
  /**
   * Run with whatever of this the host can enforce and report what was
   * dropped in `unenforced`. The default refuses.
   */
  bestEffort?: boolean;
}

/** One control, and whether this host enforces it for this request. */
export interface ControlReport {
  control: string;
  enforced: boolean;
  /** Why not, when it is not. */
  reason?: string;
}

/** A run that happened. */
export interface Result {
  version: 1;
  /** The workload's exit code; null when a signal ended it. */
  exitCode: number | null;
  /** The signal that ended it, on Unix. */
  signal: number | null;
  /** The limit that killed it, such as `wall-clock deadline`; null when none did. */
  killedBy: string | null;
  /** Standard output as text. Bytes that were not UTF-8 are replaced here. */
  stdout: string;
  stderr: string;
  /** Standard output exactly as written. */
  stdoutBytes: Buffer;
  stderrBytes: Buffer;
  /** Controls the request asked for that were dropped under `bestEffort`. */
  unenforced: string[];
  backend: string;
  os: string;
  controls: ControlReport[];
}

/** What `hm sandbox exec` answers when the run did not happen. */
export interface Refusal {
  version: 1;
  error: {
    kind: 'invalid' | 'unsupported' | 'spawn' | 'confinement' | 'runtime';
    message: string;
  };
  backend?: string;
  os?: string;
  controls?: ControlReport[];
}

export interface Options {
  /** The `hm` binary. By default `HM_BIN`, then `hm` on `PATH`. */
  hm?: string;
}

/** `hm` could not be run, or did not answer as `hm sandbox exec` does. */
export class SandboxError extends Error {}

/** The run did not happen. Nothing ran. */
export class SandboxRefused extends SandboxError {
  kind: Refusal['error']['kind'];
  response: Refusal;
}

/**
 * Run one request. Resolves for any run that happened, whatever the
 * workload's exit; rejects with `SandboxRefused` when it did not happen and
 * with `SandboxError` when `hm` could not be used.
 */
export function run(request: Request, options?: Options): Promise<Result>;
