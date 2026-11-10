import {
  chmodSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { createServer, type Server } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createContainer } from '../src/index.mjs';

let dir: string;
let server: Server;
let url: string;
const calls: Array<{ path: string; body: string }> = [];
const saved = { ...process.env };

beforeEach(async () => {
  dir = mkdtempSync(join(tmpdir(), 'browser-client-'));
  calls.length = 0;
  server = createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += String(c)));
    req.on('end', () => {
      calls.push({ path: req.url ?? '', body });
      res.setHeader('content-type', 'application/json');
      if (req.url === '/call') {
        const { method } = JSON.parse(body) as { method: string };
        res.end(
          JSON.stringify(
            method === 'broken'
              ? { ok: false, error: 'no such browser' }
              : { ok: true, result: { method, cookies: [{ name: 'a' }] } }
          )
        );
      } else res.end('{}');
    });
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  url = `http://127.0.0.1:${(server.address() as { port: number }).port}`;
});

afterEach(async () => {
  await new Promise<void>((r) => server.close(() => r()));
  rmSync(dir, { recursive: true, force: true });
  for (const k of Object.keys(process.env))
    if (!(k in saved)) delete process.env[k];
  Object.assign(process.env, saved);
});

/** A stub wf: `status` fails until `RUNNING` is set; `start` prints the mock daemon's URL and logs its argv. */
function stubWf(): void {
  const file = join(dir, 'wf');
  writeFileSync(
    file,
    `#!/bin/sh
echo "$@" >> "${dir}/argv.log"
echo "$WF_CHROMIUM" >> "${dir}/chromium.log"
case "$*" in
  *status*) [ -n "$RUNNING" ] || { echo "No browser daemon is running" >&2; exit 1; } ;;
esac
echo '{"level":30,"result":{"url":"${url}","mode":"headless"},"msg":"ok"}'
`
  );
  chmodSync(file, 0o755);
  process.env['WF_BIN'] = file;
}

const argvLog = (): string[] =>
  readFileSync(join(dir, 'argv.log'), 'utf8').trim().split('\n');

describe('createContainer', () => {
  it('given no daemon running, when a command is called, then the daemon is started once and the command is posted to /call', async () => {
    stubWf();
    const container = createContainer({
      headless: true,
      screenshotDir: '/tmp/shots',
    });

    const first = await container.navigate({ name: 'a', url: 'http://x/' });
    await container.eval({ name: 'a', expression: '1' });

    expect(first).toMatchObject({ method: 'navigate' });
    expect(argvLog()).toEqual([
      'browser --json status',
      'browser --json start --headless --screenshot-dir /tmp/shots',
    ]);
    expect(calls.map((c) => c.path)).toEqual(['/call', '/call']);
    expect(JSON.parse(calls[0]!.body)).toEqual({
      method: 'navigate',
      args: { name: 'a', url: 'http://x/' },
    });
  });

  it('given the SDK names for open, close and list, when called, then the daemon gets its own short names', async () => {
    stubWf();
    process.env['RUNNING'] = '1';
    const container = createContainer();

    await container.openBrowser({ name: 'a' });
    await container.closeBrowser({ name: 'a' });
    await container.listBrowsers();

    expect(
      calls.map((c) => (JSON.parse(c.body) as { method: string }).method)
    ).toEqual(['open', 'close', 'list']);
  });

  it('given a daemon already running, when a command is called, then it is adopted and not started again', async () => {
    stubWf();
    process.env['RUNNING'] = '1';
    const container = createContainer();

    await container.cookies({ name: 'a' });

    expect(argvLog()).toEqual(['browser --json status']);
  });

  it('given a browser executable path, when the daemon is started, then it is handed to the daemon as WF_CHROMIUM', async () => {
    stubWf();
    const container = createContainer({ headless: true });

    await container.startDaemon({ browserExecutablePath: '/opt/chromium' });

    expect(readFileSync(join(dir, 'chromium.log'), 'utf8')).toContain(
      '/opt/chromium'
    );
  });

  it('given the daemon reporting an error, when a command is called, then it rejects with that message', async () => {
    stubWf();
    process.env['RUNNING'] = '1';
    const container = createContainer();

    await expect(container.broken({ name: 'x' })).rejects.toThrow(
      'no such browser'
    );
  });

  it('given a wf that cannot start the daemon, when a command is called, then it rejects naming the failure', async () => {
    const file = join(dir, 'wf');
    writeFileSync(file, '#!/bin/sh\necho "no chromium" >&2\nexit 3\n');
    chmodSync(file, 0o755);
    process.env['WF_BIN'] = file;
    const container = createContainer();

    await expect(container.navigate({})).rejects.toThrow(
      /wf browser start failed \(exit 3\): no chromium/
    );
  });

  it('given a started daemon, when it is stopped, then /stop is posted', async () => {
    stubWf();
    process.env['RUNNING'] = '1';
    const container = createContainer();
    await container.startDaemon();

    await container.stopDaemon();

    expect(calls.map((c) => c.path)).toEqual(['/stop']);
  });
});
