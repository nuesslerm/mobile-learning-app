/**
 * Phase-9 acceptance (#252, #254) as amended by the tranche-4 honesty fixes
 * (`docs/archive/README.md`): navigating into the
 * diff fixture thread and expanding the tool group surfaces a `ToolRowDiff-*`
 * whose added and deleted rows paint distinct, non-transparent backgrounds.
 *
 * Reachable assertions, in order:
 *   1. the list renders the fixture threads with no connecting banner;
 *   2. tapping ThreadRow-fx-thread-fx-diff reaches the thread route and the
 *      diff work-log row is present;
 *   3. expanding the tool row reveals a ToolRowDiff-* whose add and del rows
 *      paint distinct, non-transparent backgrounds;
 *   4. as a second step, the same row-background contract holds on the
 *      showcase's real DS DiffView;
 *   5. no fatal page error (Minified React error, isReady) is recorded.
 *
 * Every Given/When/Then named here is executed and asserted -- nothing is
 * "gated".
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const LIST = '[data-testid="ThreadList"]';
const DIFF_ROW = '[data-testid="ThreadRow-fx-thread-fx-diff"]';
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const DIFF_TOOL_ROWS = '[data-testid^="ToolRow-"]';
const DIFF_DIFF = '[data-testid^="ToolRowDiff-"]';
const SHOWCASE_DIFF = '[data-testid="Showcase-CodingAgent-DiffView"]';
const FATAL_ERROR_RE = /Minified React error|isReady/;
// Lines the FX_DIFF_PAYLOAD fixture carries; the add side is `+`, the del `-`.
const ADD_NEEDLE = 'export const b = 3;';
const DEL_NEEDLE = 'export const a = 1;';

const server = await startFakeWsAgentServer({ port: 8787 });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function readPageErrors(evalInApp) {
  const raw = await evalInApp(
    `JSON.stringify((appDoc.defaultView && appDoc.defaultView.__pageErrors) || [])`,
  ).catch(() => '[]');
  const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
  return Array.isArray(parsed) ? parsed : [];
}

/**
 * Walk up from the text element to the nearest non-transparent background --
 * exactly the DiffView row that paints the added/deleted line.
 */
const ROW_BG_BY_CONTENT = `(needle) => {
  const view = appDoc.defaultView;
  const walker = appDoc.createTreeWalker(appDoc.body, view.NodeFilter.SHOW_TEXT, null);
  let node, textEl = null;
  while ((node = walker.nextNode())) {
    if ((node.nodeValue || '').includes(needle)) { textEl = node.parentElement; break; }
  }
  if (!textEl) return null;
  let bgEl = textEl, bg = null;
  for (let i = 0; i < 8 && bgEl; i++) {
    const c = view.getComputedStyle(bgEl).backgroundColor;
    if (c && !/rgba\\(0, 0, 0, 0\\)|transparent/.test(c)) { bg = c; break; }
    bgEl = bgEl.parentElement;
  }
  return bg ? { bg, text: textEl.textContent.slice(0, 60) } : null;
}`;

try {
  await withServedApp(
    { port: 8210, name: 'coding-agent-diff-verify', path: 'coding-agent' },
    async ({ container, name, evalInApp, tapInApp, goto }) => {
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
        fail(`a connecting/reconnecting banner is shown while WS should be open: ${JSON.stringify(listReady)}`);
        return;
      }

      // ---- The diff fixture is present in the snapshot -----------------
      const diffRow = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(DIFF_ROW)})`
      );
      if (!diffRow) {
        fail(`the diff fixture thread is missing from the list`);
        return;
      }

      // ---- (1) Navigate into fx-thread-fx-diff --------------------------
      await tapInApp(DIFF_ROW);
      let thread = null;
      for (let attempt = 0; attempt < 10; attempt++) {
        thread = await evalInApp(`(() => {
          const w = appDoc.defaultView;
          const screen = appDoc.querySelector(${JSON.stringify(THREAD_SCREEN)});
          const r = screen ? screen.getBoundingClientRect() : null;
          return {
            path: appDoc.location.pathname,
            screenBox: !!(r && r.width > 0 && r.height > 0),
            toolRows: appDoc.querySelectorAll(${JSON.stringify(DIFF_TOOL_ROWS)}).length,
          };
        })()`).catch(() => null);
        if (
          thread?.path.endsWith('/coding-agent/fx-thread-fx-diff') &&
          thread.screenBox &&
          thread.toolRows >= 1
        ) {
          break;
        }
        await sleep(400);
      }
      if (
        !thread?.path.endsWith('/coding-agent/fx-thread-fx-diff') ||
        !thread.screenBox ||
        thread.toolRows < 1
      ) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${DIFF_ROW} did not reach a thread with a tool row: ` +
            `state=${JSON.stringify(thread)} pageErrors=${JSON.stringify(pageErrors)}`
        );
        return;
      }

      // ---- (2) Expand the tool group ------------------------------------
      // The diff work-log row is collapsed by default; tapping it expands and
      // mounts the ToolRowDiff-* the thread renders from entry.diff.
      await tapInApp(DIFF_TOOL_ROWS);
      let diffRows = 0;
      for (let attempt = 0; attempt < 8; attempt++) {
        diffRows = await evalInApp(
          `appDoc.querySelectorAll(${JSON.stringify(DIFF_DIFF)}).length`
        ).catch(() => 0);
        if (diffRows >= 1) break;
        await sleep(300);
      }
      if (diffRows < 1) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `expanding the diff tool row revealed no ToolRowDiff-* row ` +
            `(diffRows=${diffRows}) pageErrors=${JSON.stringify(pageErrors)}`
        );
        return;
      }

      // ---- (3) DiffView row backgrounds on the thread -------------------
      // The add/del lines of the FX_DIFF_PAYLOAD the fixture replays must
      // paint distinct, non-transparent backgrounds on the thread's real
      // DiffView.
      const addRow = await evalInApp(
        `(() => { const s = ${ROW_BG_BY_CONTENT}; return s(${JSON.stringify(ADD_NEEDLE)}); })()`
      );
      const delRow = await evalInApp(
        `(() => { const s = ${ROW_BG_BY_CONTENT}; return s(${JSON.stringify(DEL_NEEDLE)}); })()`
      );
      if (!addRow?.bg || !delRow?.bg) {
        fail(
          `could not resolve a thread DiffView add/del row background ` +
            `(add=${JSON.stringify(addRow)}, del=${JSON.stringify(delRow)})`
        );
        return;
      }
      if (addRow.bg === delRow.bg) {
        fail(
          `thread DiffView add and del rows paint the SAME background (${addRow.bg}) -- ` +
            `the rows must differ so the user can tell the sides apart`
        );
        return;
      }
      if (
        /rgba\\(0, 0, 0, 0\\)|transparent/i.test(addRow.bg) ||
        /rgba\\(0, 0, 0, 0\\)|transparent/i.test(delRow.bg)
      ) {
        fail(
          `a thread DiffView row background is transparent (add=${addRow.bg}, del=${delRow.bg}) -- ` +
            `the painted rows must be opaque`
        );
        return;
      }

      // ---- (4) Second step: showcase DiffView rows ----------------------
      await goto('showcase');
      let diffReady = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        await sleep(400);
        diffReady = await evalInApp(
          `!!appDoc.querySelector(${JSON.stringify(SHOWCASE_DIFF)})`
        );
        if (diffReady) break;
      }
      if (!diffReady) {
        fail(`the showcase DiffView never rendered after goto('showcase')`);
        return;
      }
      const showcaseAdd = await evalInApp(
        `(() => { const s = ${ROW_BG_BY_CONTENT}; return s(${JSON.stringify('const name = "world"')}); })()`
      );
      const showcaseDel = await evalInApp(
        `(() => { const s = ${ROW_BG_BY_CONTENT}; return s(${JSON.stringify('return "hi"')}); })()`
      );
      if (!showcaseAdd?.bg || !showcaseDel?.bg) {
        fail(
          `could not resolve a showcase DiffView add/del row background ` +
            `(add=${JSON.stringify(showcaseAdd)}, del=${JSON.stringify(showcaseDel)})`
        );
        return;
      }
      if (showcaseAdd.bg === showcaseDel.bg) {
        fail(
          `showcase DiffView add and del rows paint the SAME background (${showcaseAdd.bg})`
        );
        return;
      }
      if (
        /rgba\\(0, 0, 0, 0\\)|transparent/i.test(showcaseAdd.bg) ||
        /rgba\\(0, 0, 0, 0\\)|transparent/i.test(showcaseDel.bg)
      ) {
        fail(
          `a showcase DiffView row background is transparent ` +
            `(add=${showcaseAdd.bg}, del=${showcaseDel.bg})`
        );
        return;
      }

      // ---- (5) No fatal page errors -------------------------------------
      const fatal = (await readPageErrors(evalInApp)).filter((e) =>
        FATAL_ERROR_RE.test(e)
      );
      if (fatal.length > 0) {
        fail(`fatal page errors at end of run: ${JSON.stringify(fatal)}`);
        return;
      }

      const shot = await container.screenshot({ name });
      if (shot?.path) console.log(`screenshot (thread + showcase DiffView): ${shot.path}`);

      console.log(
        `ok: list rendered ${listReady.rowCount} fixture threads with no connecting banner; ` +
          `tapping ${DIFF_ROW} reached /coding-agent/fx-thread-fx-diff with a rendered ` +
          `ThreadScreen and ${thread.toolRows} tool row(s); expanding the tool row revealed ` +
          `${diffRows} ToolRowDiff-* whose add row ("${addRow.text.trim()}") paints ${addRow.bg} ` +
          `and del row ("${delRow.text.trim()}") paints ${delRow.bg} -- distinct and ` +
          `non-transparent; the showcase DiffView add row paints ${showcaseAdd.bg} and del row ` +
          `paints ${showcaseDel.bg}; no Minified-React-error/isReady page errors.`
      );
    }
  );
} finally {
  await server.stop();
}
