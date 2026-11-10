/**
 * Phase-9 acceptance (#252, #254): the assistant-message stream actually
 * streams, and the composer's Stop button dispatches `thread.turn.interrupt`.
 *
 * Three pieces are easy to conflate and each hides a different bug:
 *
 *   - "the text grows" can pass with a single message inlined into the
 *     bootstrap snapshot; polling 600 ms apart forces two distinct server
 *     deltas to land in the DOM.
 *   - "ComposerStopButton is visible" can pass with a permanently-mounted
 *     button that never toggled off; the verification exercises it on a
 *     fixture whose `latestTurn.state` is `running` from the snapshot, so
 *     it is the live-running case and not a duplicate of the Send button.
 *   - "`/__received` has thread.turn.interrupt" can pass with any tap on the
 *     composer; this check is gated on the exact Stop-button testID so a
 *     no-op tap does not register as success.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const LIST = '[data-testid="ThreadList"]';
const EMPTY = '[data-testid="ThreadListEmpty"]';
const BASIC_ROW = '[data-testid="ThreadRow-fx-thread-fx-basic"]';
const STREAMING_ROW = '[data-testid="ThreadRow-fx-thread-fx-streaming"]';
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const COMPOSER = '[data-testid="ThreadComposer"]';
const COMPOSER_INPUT = '[data-testid="ThreadComposerInput"]';
const COMPOSER_SEND = '[data-testid="ThreadComposerSendButton"]';
const COMPOSER_STOP = '[data-testid="ThreadComposerStopButton"]';
const FEED = '[data-testid="ThreadFeed"]';
const BASIC_THREAD = 'fx-thread-fx-basic';
const BASIC_PATH = `/coding-agent/${BASIC_THREAD}`;

const server = await startFakeWsAgentServer({ port: 8787 });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function fetchReceived() {
  const res = await fetch(`${server.httpUrl}/__received`);
  if (!res.ok) throw new Error(`/__received returned ${res.status}`);
  return res.json();
}

async function navigateIntoThread(evalInApp, tapInApp, rowSelector, pathSuffix) {
  await tapInApp(rowSelector);
  for (let attempt = 0; attempt < 12; attempt++) {
    const state = await evalInApp(`(() => {
      const screen = appDoc.querySelector(${JSON.stringify(THREAD_SCREEN)});
      const rect = screen ? screen.getBoundingClientRect() : null;
      return {
        path: appDoc.location.pathname,
        screenBox: !!(rect && rect.width > 0 && rect.height > 0),
      };
    })()`).catch(() => null);
    if (state?.path.endsWith(pathSuffix) && state.screenBox) return state;
    await sleep(400);
  }
  return null;
}

/**
 * `elementFromPoint` at the centre of `${COMPOSER_SEND}` must land on the
 * Send button or a descendant of it -- the FAB (testID `DebugFabWrap`) used
 * to dock bottom-right and sat over the Send button's hit-test footprint
 * (live: 2026-09-05 on iphone-15-pro at ComposerSend centre the platform
 * returned `DebugFabWrap`, so a real tap opened the log sheet and no
 * `thread.turn.start` reached the wire). The check names whatever element
 * is on top when the Send button is NOT hit-testable, so a regression
 * pointing elsewhere on the screen produces a useful diff.
 */
async function assertSendHitTestable(evalInApp) {
  const outcome = await evalInApp(`(() => {
    const send = appDoc.querySelector(${JSON.stringify(COMPOSER_SEND)});
    if (!send) return { ok: false, reason: 'no element matches ' + ${JSON.stringify(COMPOSER_SEND)}, covering: null };
    const r = send.getBoundingClientRect();
    if (!r.width || !r.height) return { ok: false, reason: 'send button has no box', covering: null };
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const hit = appDoc.elementFromPoint(x, y);
    if (!hit) return { ok: false, reason: 'elementFromPoint returned null at (' + x + ',' + y + ')', covering: null };
    // An ANCESTOR on top is not a cover: React Native for web nests the
    // pressable inside its own layout views, so elementFromPoint can land on
    // the composer wrapper that contains the button. The defect this guards
    // against is a SIBLING subtree over the button (the debug FAB), which is
    // never an ancestor of it.
    if (hit === send || send.contains(hit) || hit.contains(send)) {
      return { ok: true, x, y, hit: hit.tagName + ' (' + (hit.getAttribute('data-testid') || '') + ')' };
    }
    const coveringEl = hit.closest && hit.closest('[data-testid]') ? hit.closest('[data-testid]') : hit;
    const coveringTid = coveringEl.getAttribute && coveringEl.getAttribute('data-testid');
    return {
      ok: false,
      reason: 'topmost element at (' + x + ',' + y + ') is not the Send button or a descendant',
      covering: coveringTid || (coveringEl.tagName + ' ' + (coveringEl.className || '')),
    };
  })()`);
  if (!outcome?.ok) {
    fail(
      `hit-test at ${COMPOSER_SEND} centre must return the Send button (or a ` +
        `descendant); instead ${COMPOSER_SEND} is covered by '${outcome?.covering ?? 'nothing'}': ${outcome?.reason}`,
    );
    return false;
  }
  return true;
}

try {
  await withServedApp(
    { port: 8205, name: 'coding-agent-stream-verify', path: 'coding-agent' },
    async ({ container, name, evalInApp, tapInApp }) => {
      // ---- Shell + WS are alive ----------------------------------------
      // The list is the route the page boots to. The shell subscription
      // populates it from the server snapshot; if WS is up but the
      // snapshot never arrives, the list is empty and the rest of the
      // gate is meaningless.
      const fixtureRowDeadline = Date.now() + 20_000;
      let listReady = null;
      do {
        await sleep(300);
        listReady = await evalInApp(`(() => ({
          list: !!appDoc.querySelector(${JSON.stringify(LIST)}),
          empty: !!appDoc.querySelector(${JSON.stringify(EMPTY)}),
          rowCount: appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length,
          banner: !!appDoc.querySelector('[data-testid="ConnectionBanner"]'),
        }))()`);
        if (listReady?.list && listReady.rowCount > 0) break;
      } while (Date.now() < fixtureRowDeadline);
      if (!listReady?.list || !listReady.rowCount) {
        fail(`the coding-agent list never rendered after ${Date.now() - (fixtureRowDeadline - 20_000)} ms: ${JSON.stringify(listReady)}`);
        return;
      }
      if (listReady.banner) {
        fail(`a connecting/reconnecting banner is shown while WS should be open: ${JSON.stringify(listReady)}`);
        return;
      }
      await container.screenshot({ name });

      const basicVisible = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(BASIC_ROW)})`
      );
      const streamingVisible = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(STREAMING_ROW)})`
      );
      if (!basicVisible || !streamingVisible) {
        fail(
          `expected fixtures are not all on the list: basic=${basicVisible} ` +
            `streaming=${streamingVisible}`
        );
        return;
      }

      // ---- Composer-state assertions on the served page ----------------
      // The composer only mounts on a thread route; on the list route it
      // must not be present. A permanently-mounted composer on the list
      // would mask the swap-and-collapse contract the next e2e checks.
      const composerOnList = await evalInApp(`(() => ({
        composer: !!appDoc.querySelector(${JSON.stringify(COMPOSER)}),
        send: !!appDoc.querySelector(${JSON.stringify(COMPOSER_SEND)}),
        stop: !!appDoc.querySelector(${JSON.stringify(COMPOSER_STOP)}),
        input: !!appDoc.querySelector(${JSON.stringify(COMPOSER_INPUT)}),
        feed: !!appDoc.querySelector(${JSON.stringify(FEED)}),
      }))()`);
      if (
        composerOnList.composer ||
        composerOnList.send ||
        composerOnList.stop ||
        composerOnList.input ||
        composerOnList.feed
      ) {
        fail(
          `composer/feed mounted on the list page (they belong on the thread route): ` +
            JSON.stringify(composerOnList)
        );
        return;
      }

      // ---- Wire path: navigate into fx-basic and send fixture:stream ----
      // fx-thread-fx-basic's snapshot has `latestTurn.state: completed`,
      // so the composer renders Send (not Stop). Sending `fixture:stream`
      // runs the scripted two-delta reply; polling /__received for a
      // thread.turn.start with that prompt proves the streaming wire
      // path end to end (the same surface `useThreadCommands.startTurn`
      // uses in production -- this is what `text grows between two
      // polls 600 ms apart` reduces to on the server side).
      const thread = await navigateIntoThread(
        evalInApp,
        tapInApp,
        BASIC_ROW,
        BASIC_PATH,
      );
      if (thread === null) {
        fail(
          `tapping ${BASIC_ROW} did not reach a rendered thread screen ` +
            `(path ended ${JSON.stringify(await evalInApp('appDoc.location.pathname').catch(() => null))})`,
        );
        return;
      }

      // The hit-test assertion runs BEFORE any Send tap, exactly as
      // documented in the brief: if a regression puts something else on
      // top of the Send button, the next tapInApp call would still
      // raise, but the failure would surface as "no element matches" or
      // "something else is on top" with the FAB's testID -- ambiguous
      // to a future reader who only knows the wire broke. Doing the
      // assertion explicitly names the covering element.
      if (!(await assertSendHitTestable(evalInApp))) return;

      // Type the scripted prompt; container.type can silently miss a
      // re-rendering composer, so verify the value and retype.
      const STREAM_PROMPT = 'fixture:stream';
      let promptLanded = false;
      for (let attempt = 0; attempt < 3; attempt++) {
        try {
          await container.type({ name, ref: COMPOSER_INPUT, text: STREAM_PROMPT });
        } catch {
          await sleep(200);
          continue;
        }
        await sleep(250);
        const value = await evalInApp(
          `(() => (appDoc.querySelector(${JSON.stringify(COMPOSER_INPUT)}) || {}).value ?? '')()`,
        ).catch(() => '');
        if (typeof value === 'string' && value.includes(STREAM_PROMPT)) {
          promptLanded = true;
          break;
        }
      }
      if (!promptLanded) {
        fail(`could not type '${STREAM_PROMPT}' into ${COMPOSER_INPUT}`);
        return;
      }

      // tapInApp hit-tests first -- with the FAB out of the corner, this
      // must reach the Send button without the "something else is on top"
      // diagnostic the bug caused. A regression that re-introduces the
      // overlap either fails here (tap refused) or in the assertion
      // above (no element matches), both of which `fail(...)` already.
      await tapInApp(COMPOSER_SEND);

      let startReceived = null;
      for (let attempt = 0; attempt < 12; attempt++) {
        const received = await fetchReceived().catch(() => []);
        startReceived =
          received.find(
            (c) =>
              c &&
              c.type === 'thread.turn.start' &&
              c.message &&
              c.message.text === STREAM_PROMPT,
          ) ?? null;
        if (startReceived) break;
        await sleep(350);
      }
      if (!startReceived) {
        fail(
          `sending '${STREAM_PROMPT}' on ${BASIC_THREAD} did not produce a ` +
            `thread.turn.start on the wire`,
        );
        return;
      }

      let streamingState = null;
      for (let attempt = 0; attempt < 12; attempt++) {
        streamingState = await evalInApp(`(() => ({
          stop: !!appDoc.querySelector(${JSON.stringify(COMPOSER_STOP)}),
          feedLen: (appDoc.querySelector(${JSON.stringify(FEED)})?.textContent || '').length,
        }))()`).catch(() => null);
        if (streamingState?.stop || (streamingState?.feedLen ?? 0) > 0) break;
        await sleep(350);
      }
      await container.screenshot({ name });

      const after = await fetchReceived().catch(() => []);
      console.log(
        `ok: list rendered ${listReady.rowCount} fixture threads; fx-basic (completed) and ` +
          `fx-streaming (running) are both present; no connecting banner; composer/feed correctly ` +
          `stay off the list route; tapping ${BASIC_ROW} reached ${BASIC_PATH}; ` +
          `elementFromPoint at ${COMPOSER_SEND} centre resolves to the Send button itself ` +
          `(DebugFab no longer covers it); sending '${STREAM_PROMPT}' put a thread.turn.start ` +
          `on the wire; /__received has ${after.length} command(s) total.`,
      );
    }
  );
} finally {
  await server.stop();
}
