import { spawnSync } from 'node:child_process';

/** The SDK's long names for the daemon's `open`, `close` and `list`. */
const DAEMON_METHOD = {
  openBrowser: 'open',
  closeBrowser: 'close',
  listBrowsers: 'list',
};

/**
 * The daemon of the Rust wf (`wf browser`), driven from node.
 *
 * `createContainer()` returns an object whose every other method is a daemon command: `container.navigate({ name, url })`
 * posts `{ method: 'navigate', args }` to the daemon's `/call` and resolves with its result, or rejects with its error. The
 * daemon is started on first use (`wf browser start`) unless one is already running for this checkout.
 *
 * @param {import('./index.d.mts').ContainerOptions} [options]
 * @returns {import('./index.d.mts').Container}
 */
export function createContainer(options = {}) {
  /** @type {string | undefined} */
  let url;

  const wf = () => process.env['WF_BIN'] || 'wf';

  /** @param {string[]} args @param {NodeJS.ProcessEnv} [env] */
  function runWf(args, env = process.env) {
    const result = spawnSync(wf(), ['browser', '--json', ...args], {
      encoding: 'utf8',
      env,
    });
    if (result.error) {
      throw new Error(`cannot run "${wf()}": ${result.error.message}`);
    }
    return result;
  }

  /** The daemon's URL from a `--json` reply: the last line that parses and carries `result.url`. */
  function urlFrom(stdout) {
    for (const line of stdout.trim().split('\n').reverse()) {
      try {
        const found = JSON.parse(line)?.result?.url;
        if (typeof found === 'string') return found;
      } catch {
        // not a JSON line
      }
    }
    return undefined;
  }

  return new Proxy(
    {
      /** Starts the daemon (or adopts the one already running) and returns its URL. */
      async startDaemon({ browserExecutablePath } = {}) {
        const running = runWf(['status']);
        const existing =
          running.status === 0 ? urlFrom(running.stdout) : undefined;
        if (existing) {
          url = existing;
          return { url };
        }
        const args = ['start'];
        if (options.headless) args.push('--headless');
        if (options.screenshotDir)
          args.push('--screenshot-dir', options.screenshotDir);
        const started = runWf(
          args,
          browserExecutablePath
            ? { ...process.env, WF_CHROMIUM: browserExecutablePath }
            : process.env
        );
        const found =
          started.status === 0 ? urlFrom(started.stdout) : undefined;
        if (!found) {
          throw new Error(
            `wf browser start failed (exit ${started.status}): ${started.stderr.trim() || started.stdout.trim()}`
          );
        }
        url = found;
        return { url };
      },

      /** Stops the daemon for this checkout. */
      async stopDaemon() {
        if (!url) return;
        await fetch(`${url}/stop`, { method: 'POST' }).catch(() => undefined);
        url = undefined;
      },
    },
    {
      get(target, method) {
        if (typeof method !== 'string' || method === 'then') return undefined;
        if (method in target) return target[method];
        return async (args = {}) => {
          if (!url) await target.startDaemon();
          const response = await fetch(`${url}/call`, {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({
              method: DAEMON_METHOD[method] ?? method,
              args,
            }),
          });
          const reply = await response.json();
          if (reply.ok !== true) {
            throw new Error(reply.error ?? `browser command ${method} failed`);
          }
          return reply.result;
        };
      },
    }
  );
}
