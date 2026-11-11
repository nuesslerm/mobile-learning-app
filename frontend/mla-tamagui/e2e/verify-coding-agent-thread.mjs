/**
 * Phase-9 acceptance (#252, #254) as amended by the tranche-4 honesty fixes
 * (`docs/archive/README.md`): the thread route is
 * actually reached and rendered, the header back control returns to the list,
 * `NewThreadButton` puts a `thread.create` on the wire and navigates to a
 * genuinely new id, and no fatal page error (`Minified React error`,
 * `isReady`) is recorded. Every Given/When/Then named here is executed and
 * asserted -- nothing is "gated".
 *
 * The WebSocket is the fake agent server in
 * `frontend/mla-tamagui/features/coding-agent/src/testing/node/fakeWsAgentServer.mjs` on port 8787. It
 * replays the fixtures under `fx-thread-<fixture>` ids, so the served list
 * shows ten rows; `fx-thread-fx-basic` is the completed two-message
 * conversation this check navigates into. `/__received` records every command
 * the client dispatches, which is what the `NewThreadButton` step asserts on.
 *
 * One step survives from the original check: the reload-persistence probe --
 * after the back-navigation, a fresh `goto` re-mounts the list and the
 * fixture rows the fake server seeds must still be there.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const LIST = '[data-testid="ThreadList"]';
const EMPTY = '[data-testid="ThreadListEmpty"]';
const BASIC_ROW = '[data-testid="ThreadRow-fx-thread-fx-basic"]';
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const THREAD_FEED = '[data-testid="ThreadFeed"]';
const BACK = '[data-testid="ThreadScreenHeaderBack"]';
const NEW_THREAD = '[data-testid="NewThreadButton"]';
// Feed-entry message testIDs the impl actually exposes: the `UserMessage-*`
// card on user messages and the `AssistantMessage*` reasoning/caret markers
// on assistant messages. A plain assistant message carries no container
// testID, so the assistant side is asserted by its rendered text below.
const FEED_MESSAGE =
  '[data-testid^="UserMessage-"], [data-testid^="AssistantMessage"]';
const FX_BASIC_ASSISTANT_REPLY = 'Hello there, friend, greetings, hi.';
const FATAL_ERROR_RE = /Minified React error|isReady/;

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
    { port: 8204, name: 'coding-agent-thread-verify', path: 'coding-agent' },
    async ({ container, name, evalInApp, tapInApp, goto }) => {
      // ---- Given: the list shows the fixture rows -----------------------
      // The WS handshake + shell snapshot are not instantaneous; a bare
      // presence check is a trap because both `ThreadListEmpty` and
      // `ThreadList` are mounted by React before the WS arrives, and only
      // geometry says which one is actually visible.
      const fixtureRowDeadline = Date.now() + 20_000;
      let initial = null;
      do {
        await sleep(250);
        initial = await evalInApp(`(() => {
          const w = appDoc.defaultView;
          const list = appDoc.querySelector(${JSON.stringify(LIST)});
          const listRect = list ? list.getBoundingClientRect() : null;
          const rowIds = [...appDoc.querySelectorAll('[data-testid^="ThreadRow-"]')]
            .map((e) => e.getAttribute('data-testid').replace(/^ThreadRow-/, ''))
            .sort();
          return {
            empty: !!appDoc.querySelector(${JSON.stringify(EMPTY)}),
            listVisible: !!(listRect && listRect.width > 0 && listRect.height > 0
              && listRect.bottom > 0 && listRect.top < w.innerHeight),
            rowCount: rowIds.length,
            rowIds,
          };
        })()`).catch(() => null);
        if (initial?.listVisible && initial.rowCount > 0) break;
      } while (Date.now() < fixtureRowDeadline);
      if (!initial?.listVisible || initial.rowCount === 0) {
        fail(
          `the coding-agent list never rendered its fixture rows after ${Date.now() - (fixtureRowDeadline - 20_000)} ms: ${JSON.stringify(initial)}`,
        );
        return;
      }
      const fixtureIds = initial.rowIds;
      if (fixtureIds.length !== 10) {
        fail(
          `expected the 10 fixture threads on the list, saw ${fixtureIds.length}: ` +
            `[${fixtureIds.join(',')}]`,
        );
        return;
      }
      if (!fixtureIds.includes('fx-thread-fx-basic')) {
        fail(
          `the fx-basic fixture thread is not on the list: [${fixtureIds.join(',')}]`,
        );
        return;
      }
      await container.screenshot({ name });

      // ---- (1) When ThreadRow-fx-thread-fx-basic is tapped ---------------
      // Then, within 4s, the pathname ends with the thread route, ThreadScreen
      // has a non-zero box, the feed rendered at least one message entry the
      // impl marks with a testID, and the assistant's reply text is present.
      await tapInApp(BASIC_ROW);
      let thread = null;
      for (let attempt = 0; attempt < 10; attempt++) {
        thread = await evalInApp(`(() => {
          const w = appDoc.defaultView;
          const screen = appDoc.querySelector(${JSON.stringify(THREAD_SCREEN)});
          const r = screen ? screen.getBoundingClientRect() : null;
          const feed = appDoc.querySelector(${JSON.stringify(THREAD_FEED)});
          return {
            href: appDoc.location.href,
            path: appDoc.location.pathname,
            screenBox: !!(r && r.width > 0 && r.height > 0),
            messageCount: appDoc.querySelectorAll(${JSON.stringify(FEED_MESSAGE)}).length,
            assistantReply: !!(feed && feed.textContent.includes(${JSON.stringify(FX_BASIC_ASSISTANT_REPLY)})),
          };
        })()`).catch(() => null);
        if (
          thread?.path.endsWith('/coding-agent/fx-thread-fx-basic') &&
          thread.screenBox &&
          thread.messageCount >= 1 &&
          thread.assistantReply
        ) {
          break;
        }
        await sleep(400);
      }
      const threadOk =
        thread?.path.endsWith('/coding-agent/fx-thread-fx-basic') &&
        thread.screenBox &&
        thread.messageCount >= 1 &&
        thread.assistantReply;
      if (!threadOk) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${BASIC_ROW} did not reach a rendered thread screen: ` +
            `state=${JSON.stringify(thread)} pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }
      // The next step's back-navigation is a full page load that discards the
      // current document's error log, so the thread-mount errors are checked
      // here, while they are still in the same document they occurred in.
      const threadMountErrors = (await readPageErrors(evalInApp)).filter((e) =>
        FATAL_ERROR_RE.test(e),
      );
      if (threadMountErrors.length > 0) {
        fail(
          `the thread screen raised fatal page errors while mounting: ` +
            `${JSON.stringify(threadMountErrors)}`,
        );
        return;
      }
      await container.screenshot({ name });

      // ---- (2) When the header back control is tapped --------------------
      // Then the pathname is back to /coding-agent and the rows are still
      // there. Back is a real page load on a static export, so the eval can
      // transiently hit a detached frame -- poll until the new document is
      // the list again.
      await tapInApp(BACK);
      let back = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        back = await evalInApp(`(() => ({
          href: appDoc.location.href,
          path: appDoc.location.pathname,
          rowCount: appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length,
        }))()`).catch(() => null);
        if (back && /\/coding-agent$/.test(back.path) && back.rowCount > 0) break;
        await sleep(400);
      }
      if (!back || !/\/coding-agent$/.test(back.path) || back.rowCount === 0) {
        fail(
          `the header back control did not return to the list with its rows: ` +
            `${JSON.stringify(back)}`,
        );
        return;
      }

      // ---- Reload persistence (kept from the original check) -------------
      await goto('coding-agent');
      let reload = null;
      for (let attempt = 0; attempt < 16; attempt++) {
        await sleep(400);
        reload = await evalInApp(`(() => ({
          list: !!appDoc.querySelector(${JSON.stringify(LIST)}),
          rowIds: [...appDoc.querySelectorAll('[data-testid^="ThreadRow-"]')]
            .map((e) => e.getAttribute('data-testid').replace(/^ThreadRow-/, ''))
            .sort(),
        }))()`).catch(() => null);
        if (reload?.list && reload.rowIds.length > 0) break;
      }
      if (!reload?.list || reload.rowIds.length === 0) {
        fail(`the list did not re-render after a fresh reload: ${JSON.stringify(reload)}`);
        return;
      }
      const survived = fixtureIds.every((id) => reload.rowIds.includes(id));
      if (!survived) {
        fail(
          `reload dropped threads the fake server knew about: before=[${fixtureIds.join(',')}] ` +
            `reload=[${reload.rowIds.join(',')}]`,
        );
        return;
      }

      // ---- (3) When NewThreadButton is tapped ----------------------------
      // Then, within 4s, the pathname is /coding-agent/<newId> where newId is
      // NOT one of the fixture ids, and /__received holds exactly one
      // thread.create carrying that newId.
      await tapInApp(NEW_THREAD);
      let created = null;
      for (let attempt = 0; attempt < 10; attempt++) {
        created = await evalInApp(`(() => ({
          href: appDoc.location.href,
          path: appDoc.location.pathname,
        }))()`).catch(() => null);
        if (created && /\/coding-agent\/[^/]+$/.test(created.path)) break;
        await sleep(400);
      }
      const newMatch = created && /\/coding-agent\/([^/]+)$/.exec(created.path);
      const newId = newMatch ? newMatch[1] : null;
      if (!newId) {
        fail(
          `tapping NewThreadButton did not navigate to a new thread route: ` +
            `${JSON.stringify(created)}`,
        );
        return;
      }
      if (fixtureIds.includes(newId)) {
        fail(
          `NewThreadButton navigated to an existing fixture id ${newId} -- expected a NEW thread id`,
        );
        return;
      }
      let received = [];
      try {
        received = await fetchReceived();
      } catch (err) {
        fail(`/__received could not be read: ${err?.message ?? err}`);
        return;
      }
      const creates = received.filter((c) => c && c.type === 'thread.create');
      if (creates.length !== 1) {
        fail(
          `expected exactly one thread.create on the wire, saw ${creates.length}: ` +
            `${JSON.stringify(received)}`,
        );
        return;
      }
      if (creates[0].threadId !== newId) {
        fail(
          `thread.create carried threadId=${creates[0].threadId} but the app ` +
            `navigated to ${newId}: ${JSON.stringify(creates[0])}`,
        );
        return;
      }

      // ---- (4) No fatal page errors may survive the run ------------------
      const pageErrors = await readPageErrors(evalInApp);
      const fatal = pageErrors.filter((e) => FATAL_ERROR_RE.test(e));
      if (fatal.length > 0) {
        fail(`fatal page errors at end of run: ${JSON.stringify(fatal)}`);
        return;
      }

      console.log(
        `ok: list rendered the 10 fixture rows; tapping ${BASIC_ROW} reached ` +
          `/coding-agent/fx-thread-fx-basic with a non-zero ThreadScreen box, ` +
          `${thread.messageCount} feed message testID(s) and the assistant reply ` +
          `rendered; ${BACK} returned to the list (path=${back.path}, ${back.rowCount} ` +
          `rows); a fresh reload kept [${reload.rowIds.join(',')}]; NewThreadButton ` +
          `navigated to new id ${newId} and the wire recorded exactly one ` +
          `thread.create for it; no Minified-React-error/isReady page errors.`
      );
    }
  );
} finally {
  await server.stop();
}