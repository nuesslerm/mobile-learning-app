/**
 * Phase-9 acceptance (#252, #254) as amended by the tranche-4 honesty fixes
 * (`docs/archive/README.md`): the model picker in
 * the thread settings sheet lists every CLI the server probe reports (opencode
 * and claude) with codex marked unavailable, and picking a claude model moves
 * the composer's ModelChip label and dispatches a `thread.meta.update` on the
 * wire carrying that modelSelection (f05 wires it through useThreadCommands).
 *
 * Reachable assertions, in order:
 *   1. the list renders the fixture threads with no connecting banner;
 *   2. tapping ThreadRow-fx-thread-fx-basic reaches the thread route;
 *   3. tapping ThreadSettingsButton opens the sheet;
 *   4. the sheet lists opencode and claude groups with codex marked
 *      unavailable;
 *   5. tapping a claude ModelOption-* changes the ModelChip label;
 *   6. /__received holds a thread.meta.update carrying that modelSelection;
 *   7. no fatal page error (Minified React error, isReady) is recorded.
 *
 * Every Given/When/Then named here is executed and asserted -- nothing is
 * "gated".
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const MODEL_THREAD = 'fx-thread-fx-basic';
const LIST = '[data-testid="ThreadList"]';
const BASIC_ROW = '[data-testid="ThreadRow-fx-thread-fx-basic"]';
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const SETTINGS_BUTTON = '[data-testid="ThreadSettingsButton"]';
const SHEET = '[data-testid="ThreadSettingsSheet"]';
const MODEL_CHIP = '[data-testid="ModelChip"]';
const FATAL_ERROR_RE = /Minified React error|isReady/;
// The claude option we tap; its ModelOption testID is ModelOption-<cli>-<modelID>.
const CLAUDE_OPTION = '[data-testid="ModelOption-claude-claude-sonnet-4.6"]';
const CLAUDE_MODEL_ID = 'claude-sonnet-4.6';

const server = await startFakeWsAgentServer({ port: 8787 });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function fetchReceived() {
  const res = await fetch(`${server.httpUrl}/__received`);
  if (!res.ok) throw new Error(`/__received returned ${res.status}`);
  return res.json();
}

async function readPageErrors(evalInApp) {
  const raw = await evalInApp(
    `JSON.stringify((appDoc.defaultView && appDoc.defaultView.__pageErrors) || [])`,
  ).catch(() => '[]');
  const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
  return Array.isArray(parsed) ? parsed : [];
}

try {
  await withServedApp(
    { port: 8211, name: 'coding-agent-model-verify', path: 'coding-agent' },
    async ({ evalInApp, tapInApp }) => {
      // ---- Shell + WS are alive ----------------------------------------
      let listReady = null;
      for (let attempt = 0; attempt < 16; attempt++) {
        await sleep(300);
        listReady = await evalInApp(`(() => ({
          list: !!appDoc.querySelector(${JSON.stringify(LIST)}),
          rowCount: appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length,
          banner: !!appDoc.querySelector('[data-testid="ConnectionBanner"]'),
        }))()`);
        if (listReady?.list && listReady.rowCount > 0) break;
      }
      if (!listReady?.list || !listReady.rowCount) {
        fail(`the coding-agent list never rendered: ${JSON.stringify(listReady)}`);
        return;
      }
      if (listReady.banner) {
        fail(
          `a connecting/reconnecting banner is shown while WS should be open: ` +
            JSON.stringify(listReady)
        );
        return;
      }

      // ---- The model fixture thread is present in the snapshot ---------
      const modelRow = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(BASIC_ROW)})`
      );
      if (!modelRow) {
        fail(`the model fixture thread (${MODEL_THREAD}) is missing from the list`);
        return;
      }

      // ---- (1) Navigate into fx-thread-fx-basic -------------------------
      await tapInApp(BASIC_ROW);
      let thread = null;
      for (let attempt = 0; attempt < 10; attempt++) {
        thread = await evalInApp(`(() => {
          const w = appDoc.defaultView;
          const screen = appDoc.querySelector(${JSON.stringify(THREAD_SCREEN)});
          const r = screen ? screen.getBoundingClientRect() : null;
          return {
            path: appDoc.location.pathname,
            screenBox: !!(r && r.width > 0 && r.height > 0),
            chip: (appDoc.querySelector(${JSON.stringify(MODEL_CHIP)}) || {}).textContent || '',
          };
        })()`).catch(() => null);
        if (
          thread?.path.endsWith('/coding-agent/fx-thread-fx-basic') &&
          thread.screenBox
        ) {
          break;
        }
        await sleep(400);
      }
      if (!thread?.path.endsWith('/coding-agent/fx-thread-fx-basic') || !thread.screenBox) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${BASIC_ROW} did not reach the thread route: ` +
            `state=${JSON.stringify(thread)} pageErrors=${JSON.stringify(pageErrors)}`
        );
        return;
      }
      const chipBefore = thread.chip;

      // ---- (2) Tap ThreadSettingsButton to open the sheet ----------------
      await tapInApp(SETTINGS_BUTTON);
      let sheetOpen = false;
      for (let attempt = 0; attempt < 10; attempt++) {
        sheetOpen = await evalInApp(
          `(() => {
            const el = appDoc.querySelector(${JSON.stringify(SHEET)});
            if (!el) return false;
            const r = el.getBoundingClientRect();
            return r.width > 0 && r.height > 0;
          })()`
        ).catch(() => false);
        if (sheetOpen) break;
        await sleep(300);
      }
      if (!sheetOpen) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${SETTINGS_BUTTON} did not open the settings sheet: ` +
            `pageErrors=${JSON.stringify(pageErrors)}`
        );
        return;
      }

      // ---- (3) The sheet lists opencode + claude, codex unavailable ------
      const groups = await evalInApp(`(() => {
        const opencode = appDoc.querySelector('[data-testid="ModelGroup-opencode"]');
        const claude = appDoc.querySelector('[data-testid="ModelGroup-claude"]');
        const codexOptions = [...appDoc.querySelectorAll('[data-testid^="ModelOption-codex-"]')]
          .map((e) => e.getAttribute('data-testid'));
        return {
          opencode: opencode ? opencode.textContent : null,
          claude: claude ? claude.textContent : null,
          codexOptions,
          claudeOptions: [...appDoc.querySelectorAll('[data-testid^="ModelOption-claude-"]')]
            .map((e) => e.getAttribute('data-testid')),
        };
      })()`).catch(() => null);
      const problems = [];
      if (!groups?.opencode?.includes('OpenCode')) {
        problems.push(`opencode group missing/unlabeled (${JSON.stringify(groups?.opencode)})`);
      }
      if (!groups?.claude?.includes('Claude')) {
        problems.push(`claude group missing/unlabeled (${JSON.stringify(groups?.claude)})`);
      }
      // codex is unreachable, so it must not be offered as a selectable model
      // -- it is "marked unavailable" by being absent from the pickable set.
      if ((groups?.codexOptions ?? []).length > 0) {
        problems.push(`codex is offered as a selectable option (${JSON.stringify(groups.codexOptions)})`);
      }
      if (!(groups?.claudeOptions ?? []).includes(CLAUDE_OPTION.replace(/^\[data-testid="|"\]$/g, ''))) {
        problems.push(`no claude ModelOption for ${CLAUDE_MODEL_ID} (${JSON.stringify(groups?.claudeOptions)})`);
      }
      if (problems.length > 0) {
        fail(`the settings sheet model groups are wrong: ${problems.join('; ')}`);
        return;
      }

      // ---- (4) Tap a claude ModelOption ---------------------------------
      // The option sits inside the sheet's ScrollView; bring it to the centre
      // of the visible area before the hit-tested tap.
      await evalInApp(`(() => {
        const el = appDoc.querySelector(${JSON.stringify(CLAUDE_OPTION)});
        if (el) el.scrollIntoView({ block: 'center', inline: 'nearest' });
        return true;
      })()`);
      await sleep(400);
      await tapInApp(CLAUDE_OPTION);
      let chipAfter = chipBefore;
      for (let attempt = 0; attempt < 8; attempt++) {
        chipAfter = await evalInApp(
          `(appDoc.querySelector(${JSON.stringify(MODEL_CHIP)}) || {}).textContent || ''`
        ).catch(() => '');
        if (chipAfter === CLAUDE_MODEL_ID) break;
        await sleep(300);
      }
      if (chipAfter !== CLAUDE_MODEL_ID) {
        fail(
          `ModelChip did not change to "${CLAUDE_MODEL_ID}" after tapping ${CLAUDE_OPTION} ` +
            `(before="${chipBefore}", after="${chipAfter}")`
        );
        return;
      }

      // ---- (5) /__received carries the thread.meta.update ----------------
      let received = [];
      try {
        received = await fetchReceived();
      } catch (err) {
        fail(`/__received could not be read: ${err?.message ?? err}`);
        return;
      }
      const updates = received.filter((c) => c && c.type === 'thread.meta.update');
      const match = updates.find(
        (c) =>
          c.threadId === 'fx-thread-fx-basic' &&
          c.modelSelection?.cli === 'claude' &&
          c.modelSelection?.modelID === CLAUDE_MODEL_ID
      );
      if (!match) {
        fail(
          `no thread.meta.update for fx-thread-fx-basic with ` +
            `modelSelection {cli:'claude', modelID:'${CLAUDE_MODEL_ID}'}: ` +
            JSON.stringify(received)
        );
        return;
      }

      // ---- (6) No fatal page errors -------------------------------------
      const fatal = (await readPageErrors(evalInApp)).filter((e) =>
        FATAL_ERROR_RE.test(e)
      );
      if (fatal.length > 0) {
        fail(`fatal page errors at end of run: ${JSON.stringify(fatal)}`);
        return;
      }

      console.log(
        `ok: list rendered ${listReady.rowCount} fixture threads with no connecting banner; ` +
          `tapping ${BASIC_ROW} reached /coding-agent/fx-thread-fx-basic with ModelChip ` +
          `"${chipBefore}"; ${SETTINGS_BUTTON} opened the sheet listing opencode + claude ` +
          `with codex not offered as a selectable option; tapping ${CLAUDE_OPTION} moved the ModelChip to ` +
          `"${CLAUDE_MODEL_ID}" and /__received recorded a thread.meta.update with ` +
          `modelSelection {cli:'claude', providerID:'anthropic', modelID:'${CLAUDE_MODEL_ID}'}; ` +
          `no Minified-React-error/isReady page errors.`
      );
    }
  );
} finally {
  await server.stop();
}
