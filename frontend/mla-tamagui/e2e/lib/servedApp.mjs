/**
 * Serves the web export and drives it through @lib/browser-client (the Rust wf browser daemon).
 *
 * Extracted from verify-renders.mjs once a second checker needed the same
 * harness. CLAUDE.md forbids a standalone Playwright driver here and
 * `no-raw-playwright` enforces it, so every browser check goes through the SDK.
 */
import { spawn } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createContainer } from '@lib/browser-client';

export const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const DIST = join(APP_DIR, 'dist');
const BASE_PATH = 'mla-tamagui';

/**
 * Confirm THIS server is the one answering, not a leftover on the same port.
 *
 * A stale server on this port silently passed three runs, one of them against
 * an element that had been deliberately renamed. A detached child's bind error
 * goes nowhere, so the guard has to be on the response: serve a unique token
 * next to the export and refuse to continue until we read it back.
 */
async function assertOurServerIsAnswering(port, token) {
  for (let attempt = 0; attempt < 20; attempt++) {
    await new Promise((r) => setTimeout(r, 250));
    try {
      const res = await fetch(`http://127.0.0.1:${port}/${BASE_PATH}/verify-token.txt`);
      if (res.ok && (await res.text()).trim() === token) return;
    } catch {
      // not up yet
    }
  }
  throw new Error(
    `port ${port} is not serving this run's export -- another process is probably bound to it. ` +
      `Check with: lsof -i :${port}`
  );
}

/**
 * A prelude that resolves the APP's document from whatever frame an eval lands
 * in, exposed as `appDoc`.
 *
 * Needed because the SDK deliberately targets the DEEPEST editable frame, so
 * that keystrokes reach CodeMirror's contenteditable. The moment the code
 * editor mounts, `document` inside an eval is CodeMirror's srcdoc iframe and
 * every app-level `querySelector` returns null -- which reads as "the screen
 * did not render" rather than "you are looking at the wrong document".
 */
const STATIC_SERVER = resolve(dirname(fileURLToPath(import.meta.url)), 'staticServer.mjs');

export const APP_DOC = `const appDoc = (() => {
  let w = window;
  for (let i = 0; i < 6 && w !== w.parent; i++) {
    try {
      // Marker: the error collector below is injected ONLY into the served
      // app's HTML, so its presence identifies the app frame even when the app
      // has rendered nothing. Matching on "has a [data-testid]" instead used to
      // walk straight past a crashed app to the harness shell -- so a blank
      // screen reported "no page errors", at exactly the moment the errors were
      // the whole point.
      if (w.__pageErrors) return w.document;
    } catch (err) {
      break;
    }
    w = w.parent;
  }
  try { return w.document; } catch (err) { return document; }
})();`;

/**
 * A render error inside an Expo Router screen surfaces as an EMPTY page, not as
 * a crash: React parks the failed subtree in a Suspense boundary and the export
 * exits 0. So the collector goes in before the bundle runs -- installing one
 * from `eval()` is always too late, and reloading to fix that drops the patch
 * along with the page.
 */
function injectErrorCollector(dir, extraScript = '') {
  const script = `<script>
    window.__pageErrors = [];
    const record = (what) => { if (window.__pageErrors.length < 20) window.__pageErrors.push(what); };
    const origError = console.error;
    console.error = function (...args) {
      record(args.map((a) => (a && a.stack) ? a.stack : String(a)).join(' | ').slice(0, 900));
      return origError.apply(console, args);
    };
    window.addEventListener('error', (e) => record('uncaught: ' + (e.error && e.error.stack ? e.error.stack : e.message)));
    window.addEventListener('unhandledrejection', (e) => record('unhandled rejection: ' + String(e.reason && e.reason.stack ? e.reason.stack : e.reason)));
  </script>`;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      injectErrorCollector(full, extraScript);
    } else if (entry.name.endsWith('.html')) {
      const html = readFileSync(full, 'utf8');
      writeFileSync(full, html.replace('<head>', `<head>${extraScript}${script}`));
    }
  }
}

/**
 * `dist/` exists but holds the wrong platform's export.
 *
 * `publish:eas` runs `eas update`, which runs `expo export` -- and that writes
 * the SAME `dist/` this suite serves, with iOS bundles instead of web ones.
 * Publishing therefore silently destroys the artifact every browser check
 * depends on, and the checks then fail on a selector: the app never rendered,
 * so `tap: no element resolved for ref "[data-testid=..."` is all you see.
 *
 * That failure reads as a flaky selector and was diagnosed as one, twice, on a
 * commit where nothing about the app had changed. Naming the real cause costs
 * one readdir.
 */
function assertWebExport() {
  const platforms = join(DIST, '_expo', 'static', 'js');
  if (!existsSync(platforms)) return;
  const built = readdirSync(platforms);
  if (!built.includes('web')) {
    console.error(
      `FAIL: ${DIST} is a ${built.join('/') || 'non-web'} export, not a web one — ` +
        `publish:eas (eas update) overwrites the same dist/. ` +
        `Run: pnpm --filter mla-tamagui run export:web`
    );
    process.exit(1);
  }
  if (!existsSync(join(DIST, '404.html'))) {
    console.error(
      `FAIL: no 404.html in ${DIST} — the GitHub Pages SPA fallback that makes a ` +
        `deep link resolve. ` +
        `Run: pnpm --filter mla-tamagui run export:web`
    );
    process.exit(1);
  }
}

/**
 * @param {{ port: number, name: string, path?: string, devicePreset?: string, renderMode?: string, colorScheme?: 'light' | 'dark' | 'system', video?: boolean, videoPath?: string, audioFile?: string }} opts
 * @param {(ctx: { container: any, name: string, eval: (expr: string) => Promise<any>, evalInApp: (expr: string) => Promise<any>, tapInApp: (selector: string) => Promise<void>, goto: (path?: string) => Promise<void> }) => Promise<void>} run
 */
export async function withServedApp(opts, run) {
  if (!existsSync(DIST)) {
    console.error(`FAIL: no web export at ${DIST}. Run: pnpm --filter mla-tamagui run export:web`);
    process.exit(1);
  }
  assertWebExport();

  // app.config.js sets experiments.baseUrl, and expo-router strips that prefix
  // from location.pathname. Serving at the root would make the router see a
  // path matching no route and render Unmatched Route -- which looks exactly
  // like a Tamagui failure but is not one.
  const serveRoot = join(APP_DIR, `.serve-tmp-${opts.port}`);
  rmSync(serveRoot, { recursive: true, force: true });
  mkdirSync(serveRoot, { recursive: true });
  cpSync(DIST, join(serveRoot, BASE_PATH), { recursive: true });

  const token = `verify-${process.pid}-${Date.now()}`;
  writeFileSync(join(serveRoot, BASE_PATH, 'verify-token.txt'), token);
  injectErrorCollector(join(serveRoot, BASE_PATH));

  // Not `python3 -m http.server`: an Expo static export writes `showcase.html`
  // and expects `/showcase` to reach it, the way GitHub Pages resolves it. A
  // plain file server 404s that, and a 404 here renders as "Unmatched Route",
  // which looks exactly like a routing bug in the app.
  const server = spawn(process.execPath, [STATIC_SERVER, serveRoot, String(opts.port)], {
    stdio: 'ignore',
    detached: true,
  });

  let container;
  let browserOpen = false;
  let failed = false;
  try {
    await assertOurServerIsAnswering(opts.port, token);
    container = createContainer({ headless: true });
    await container.startDaemon({ browserExecutablePath: '/opt/pw-browsers/chromium' });
    // The device frame + mock keyboard is this repo's standing verification
    // standard (docs/goals/2026-08-22-coverage-debt-and-browser-consolidation.md),
    // not a nicety: a screenshot at desktop dimensions of a phone layout is
    // exactly as misleading as no screenshot, and the mock keyboard is what
    // raises `__MCP_KEYBOARD_VISIBLE__` -- the only signal `useKeyboardFrame`
    // has on web, and therefore the only thing that brings the editor's
    // toolbar rail up at all.
    await container.openBrowser({
      name: opts.name,
      renderMode: opts.renderMode ?? 'device-frame',
      devicePreset: opts.devicePreset ?? 'iphone-15-pro',
      keyboardMode: 'mock',
      // Not a matchMedia shim injected into the page: Playwright emulates
      // `prefers-color-scheme` at the browser context, so it is in force before
      // the app's first paint. A shim can only run after the document exists,
      // which is already too late for anything the app reads once at mount.
      colorScheme: opts.colorScheme ?? 'system',
      // Screenshots say what it looked like; a recording says what it DID --
      // which interaction raced, what flashed and went, in what order.
      ...(opts.video ? { video: true } : {}),
      ...(opts.videoPath ? { videoPath: opts.videoPath } : {}),
      ...(opts.audioFile ? { audioFile: opts.audioFile } : {}),
    });
    browserOpen = true;
    await container.navigate({
      name: opts.name,
      url: `http://127.0.0.1:${opts.port}/${BASE_PATH}/${opts.path ?? ''}?cb=${token}`,
    });
    await new Promise((r) => setTimeout(r, 1500));

    // The SDK returns the evaluated value JSON-serialized, so it must be
    // parsed before it can be inspected -- truthiness-testing the raw string
    // reports a pass for any non-empty result, including a failing one.
    const evalJson = async (expression) => {
      const { result } = await container.eval({ name: opts.name, expression });
      return typeof result === 'string' ? JSON.parse(result) : result;
    };

    /** Evaluates `expression` with `appDoc` bound to the app's own document. */
    const evalInApp = (expression) => evalJson(`(() => { ${APP_DOC} return (${expression}); })()`);

    /**
     * Clicks an app-level element, hit-testing first.
     *
     * `container.tap` cannot reach these once the code editor is mounted: the
     * SDK deliberately targets the deepest editable frame so keystrokes land in
     * CodeMirror, and an app selector then resolves to nothing at all
     * ("no element resolved for ref ..."), which reads as a missing button.
     *
     * A bare `element.click()` would sidestep that but also sidestep hit
     * testing -- and an absolutely-positioned sibling silently eating a
     * button's pointer events is exactly the class of bug this harness exists
     * to catch (it already caught one on the problem list). So the hit test is
     * kept explicitly: `elementFromPoint` at the element's centre must land on
     * the element or inside it, and the click is refused otherwise.
     *
     * Also draws the SAME visual tap-dot marker a real `container.tap()`
     * draws (via the SDK's `indicateAppTap`, using the exact (x, y) this hit
     * test already computed in the app frame's own coordinate space -- no
     * cross-frame translation needed, see that command's own doc comment).
     * Found live: every runbook video driven through this path -- the only
     * way to reach a button outside the deepest editable frame -- showed no
     * indicator at all, because this function predates `indicateAppTap` and
     * never called it. Pass `keyTint` (a single character) when the tap
     * itself stands in for a keystroke (e.g. a toolbar button that inserts a
     * symbol), to also tint that character on the mock keyboard.
     */
    const tapInApp = async (selector, { keyTint } = {}) => {
      const outcome = await evalInApp(`(() => {
        const el = appDoc.querySelector(${JSON.stringify(selector)});
        if (!el) return { ok: false, reason: 'no element matches ' + ${JSON.stringify(selector)} };
        const r = el.getBoundingClientRect();
        if (!r.width || !r.height) return { ok: false, reason: 'element has no box (not visible)' };
        const x = r.left + r.width / 2;
        const y = r.top + r.height / 2;
        const hit = appDoc.elementFromPoint(x, y);
        if (!hit || !(el === hit || el.contains(hit))) {
          return {
            ok: false,
            reason: 'something else is on top at the element centre: ' +
              (hit ? (hit.tagName + ' ' + (hit.getAttribute('data-testid') || hit.className || '')) : 'nothing'),
          };
        }
        // Dispatched rather than calling click(): the topmost node at the
        // centre of an icon button is the SVG, which has no click() method. A
        // bubbling pointer/mouse sequence reaches React's delegated listener at
        // the root either way, and matches what a real press emits.
        const opts = { bubbles: true, cancelable: true, view: appDoc.defaultView, button: 0, clientX: x, clientY: y };
        hit.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerdown', { ...opts, pointerId: 1, isPrimary: true }));
        hit.dispatchEvent(new appDoc.defaultView.MouseEvent('mousedown', opts));
        hit.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerup', { ...opts, pointerId: 1, isPrimary: true }));
        hit.dispatchEvent(new appDoc.defaultView.MouseEvent('mouseup', opts));
        hit.dispatchEvent(new appDoc.defaultView.MouseEvent('click', opts));
        return { ok: true, hit: hit.tagName, x, y };
      })()`);
      if (!outcome?.ok) throw new Error(`tapInApp(${selector}): ${outcome?.reason ?? 'unknown failure'}`);
      await container.indicateAppTap({ name: opts.name, x: outcome.x, y: outcome.y });
      if (keyTint) await container.indicateKeyPress({ name: opts.name, char: keyTint });
    };

    /**
     * Navigates to another route of the SERVED app.
     *
     * `history.back()` is not a substitute: a static export is one HTML file
     * per route, so going back is a real page load that detaches the frame the
     * eval is running in -- the call fails with "Frame was detached" rather
     * than navigating. Routing it through the harness lets it manage the
     * frame lifecycle.
     */
    const goto = async (path = '') => {
      await container.navigate({
        name: opts.name,
        url: `http://127.0.0.1:${opts.port}/${BASE_PATH}/${path}?cb=${token}-${Date.now()}`,
      });
      await new Promise((r) => setTimeout(r, 1500));
    };

    await run({ container, name: opts.name, eval: evalJson, evalInApp, tapInApp, goto });
    // closeBrowser is where the recording is finalised -- Playwright only
    // flushes the video file on context close, so there is nowhere earlier it
    // could be reported from.
    browserOpen = false;
    const closed = await container.closeBrowser({ name: opts.name });
    const video = typeof closed === 'string' ? JSON.parse(closed)?.video : closed?.video;
    if (video?.path) {
      console.log(`recording: ${video.path} (${video.format}, ${video.bytes} bytes)`);
      if (video.transcodeError) {
        console.error(`  transcode failed, kept the webm: ${video.transcodeError}`);
      }
    }
  } catch (err) {
    console.error('FAIL:', err?.message ?? err);
    failed = true;
    // Set the code at the point of failure, not at the end: the daemon
    // shutdown path was resetting a trailing process.exit() back to 0.
    process.exitCode = 1;
  } finally {
    // The browser lives in the wf daemon, not this process: a check that threw would leave
    // its name taken and the next run would fail with "already exists".
    if (browserOpen) {
      try { await container.closeBrowser({ name: opts.name }); } catch { /* the daemon is gone */ }
    }
    // Deliberately NOT calling container.stopDaemon(). startDaemon() runs the
    // daemon in THIS process and its shutdown path calls process.exit(0)
    // (use-cases/daemon/start.ts:35, data/daemon.ts:152), which overrode the
    // failure code and reported success while printing FAIL. The in-process
    // daemon dies with this process anyway.
    try { process.kill(-server.pid); } catch { /* already exited */ }
    rmSync(serveRoot, { recursive: true, force: true });
  }

  process.exit(failed || process.exitCode === 1 ? 1 : 0);
}

/** Marks the run failed without waiting for the harness to unwind. */
export function fail(message) {
  console.error(`FAIL: ${message}`);
  process.exitCode = 1;
}
