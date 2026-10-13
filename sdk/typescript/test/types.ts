// Compiled, not run: the declarations in index.d.ts must type what a caller
// writes, and refuse what a caller must not. `tsc -p test` checks this.

import { ApiError, Client, type DirectoryEntry, type ExecResult, type Prompt, type PromptEvent, type TranscriptLine } from '../index.js';

async function use(): Promise<void> {
  const hm = new Client({ endpoint: 'http://node:3980', apiKey: 'k' });
  const web = await hm.machines.create('web-01', { cpuCount: 2, diskGiB: 20 });
  await hm.machines.create('ai-01', { image: 'noble.raw', guestAgent: true, agents: ['claude', 'codex'] });
  const result: ExecResult = await web.exec('uname -a', { timeoutSecs: 10 });
  const code: number | null = result.exitCode;

  const run = await web.runs.start('make', { cwd: '/srv', env: { CI: '1' } });
  for await (const chunk of run.follow()) {
    const text: string = chunk;
    void text;
  }
  await run.signal('INT');

  const prompt = await web.prompts.start('claude', 'fix it', { unattended: true, user: 'dev' });
  for await (const event of prompt.events()) {
    const seen: PromptEvent = event;
    void seen.event?.type;
  }

  const talk = await web.conversations.create('claude', { name: 'fix', user: 'dev', unattended: true });
  const turn: Prompt = await talk.say('fix it', { env: { ANTHROPIC_API_KEY: 'k' } });
  const steered: boolean = turn.info.steered;
  const lines: TranscriptLine[] = await talk.transcript();
  const role: 'user' | 'assistant' = lines[0].role;
  await web.conversations.create('codex', { from: talk.id });
  void [steered, role];

  await hm.machines.create('desk-01', { image: 'noble.raw', guestAgent: true, desktop: { width: 1920, height: 1080 } });
  const png: Buffer = await web.desktop.screenshot();
  const [px, py]: [number, number] = await web.desktop.click(10, 20, { button: 'right', count: 2 });
  await web.desktop.key('ctrl+c');
  void [png, px, py];

  await web.files.write('/etc/x', new Uint8Array([1, 2]));
  const bytes: Buffer = await web.files.read('/etc/x');
  await web.ports.publish(8080, { proxy: true });
  await web.stop();
  await web.resize({ diskGiB: 40 });
  const snap = await web.snapshots.take('before');
  const entries: DirectoryEntry[] = await snap.listdir('/etc');
  const kind: 'file' | 'directory' | 'symlink' | 'other' = entries[0].type;
  await snap.restore();
  const written: number = await snap.download('/tmp/disk.img');
  await hm.usage({ from: '2026-10-01T00:00:00Z', by: 'team' });
  await hm.putEnvironment('web', { envVars: { A: '1' }, files: { '/etc/a': 'text' } });

  try {
    await hm.machines.get('absent');
  } catch (error) {
    if (error instanceof ApiError) {
      const status: number = error.status;
      void status;
    }
  }
  void [code, bytes, kind, written];

  // @ts-expect-error a provider that is not one
  await web.prompts.start('gpt', 'x');
  // @ts-expect-error a desktop is true or a size
  await hm.machines.create('desk-02', { desktop: false });
  // @ts-expect-error no such mouse button
  await web.desktop.click(1, 1, { button: 'fourth' });
  // @ts-expect-error a conversation is not with a custom command
  await web.conversations.create('custom');
  // @ts-expect-error a grouping that is not one
  await hm.usage({ by: 'colour' });
  // @ts-expect-error a size is a number
  await web.resize({ cpuCount: 'four' });
}

void use;
