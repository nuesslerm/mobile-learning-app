/**
 * Phase-9 acceptance (#252, #254) as amended by the tranche-4 honesty fixes
 * (`docs/archive/README.md`): the thread route is
 * actually reached, the Plan segment flips the composer placeholder, sending
 * `fixture:plan` surfaces a PlanCard on the feed, and tapping the PlanCard's
 * Implement button dispatches a `thread.turn.start` carrying the proposed
 * plan in `sourceProposedPlan` and forcing `interactionMode` to `default` so
 * the resulting build turn runs even though the composer was in Plan mode.
 * The scripted `fixture:plan` send runs on `fx-thread-fx-basic` (latestTurn
 * `completed`, so the composer renders Send) because the fx-plan fixture
 * snapshot is `latestTurn.state: running`, which swaps Send for Stop -- a
 * running turn cannot carry a new prompt. The fx-thread-fx-plan fixture is
 * still entered so its fixture PlanCard is verified. Every Given/When/Then
 * named here is executed and asserted with `fail(...)` on miss; a
 * `__pageErrors` entry containing `Minified React error` or `isReady` fails
 * the check.
 *
 * The WebSocket is the fake agent server in
 * `frontend/mla-tamagui/features/coding-agent/src/testing/node/fakeWsAgentServer.mjs` on port 8787; the
 * fx-plan fixture thread (`fx-thread-fx-plan`) carries a proposed plan already,
 * and `/__received` records every command the client dispatches. The
 * Implement assertion polls `/__received` for up to 4 s for a thread.turn.start
 * whose `sourceProposedPlan` is present and whose `interactionMode` is
 * `default`.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const LIST = '[data-testid="ThreadList"]';
const EMPTY = '[data-testid="ThreadListEmpty"]';
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const PLAN_THREAD = 'fx-thread-fx-plan';
const PLAN_ROW = `[data-testid="ThreadRow-${PLAN_THREAD}"]`;
const PLAN_PATH = `/coding-agent/${PLAN_THREAD}`;
const SCRIPTED_THREAD = 'fx-thread-fx-basic';
const SCRIPTED_ROW = `[data-testid="ThreadRow-${SCRIPTED_THREAD}"]`;
const SCRIPTED_PATH = `/coding-agent/${SCRIPTED_THREAD}`;
const PLAN_SEGMENT = '[data-testid="InteractionModeSegment-plan"]';
const COMPOSER_INPUT = '[data-testid="ThreadComposerInput"]';
const COMPOSER_SEND = '[data-testid="ThreadComposerSendButton"]';
const COMPOSER_STOP = '[data-testid="ThreadComposerStopButton"]';
const PLAN_PLACEHOLDER = 'Describe what to plan…';
const PLAN_CARD = '[data-testid^="PlanCard-"]';
const PLAN_IMPLEMENT = '[data-testid^="PlanCardImplement-"]';
const BACK = '[data-testid="ThreadScreenHeaderBack"]';
const FATAL_ERROR_RE = /Minified React error|isReady/;

const server = await startFakeWsAgentServer({ port: 8787 });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function readPageErrors(evalInApp) {
  const raw = await evalInApp(
    `JSON.stringify((appDoc.defaultView && appDoc.defaultView.__pageErrors) || [])`,
  ).catch(() => '[]');
  const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
  return Array.isArray(parsed) ? parsed : [];
}

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

async function backToList(evalInApp, tapInApp) {
  await tapInApp(BACK);
  for (let attempt = 0; attempt < 16; attempt++) {
    const state = await evalInApp(`(() => ({
      path: appDoc.location.pathname,
      rowCount: appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length,
    }))()`).catch(() => null);
    if (state && /\/coding-agent$/.test(state.path) && state.rowCount > 0) return state;
    await sleep(400);
  }
  return null;
}

/**
 * Types the scripted prompt and verifies it actually landed in the composer
 * input before the send is tapped. `container.type` can silently miss a
 * re-rendering composer, leaving the draft empty and the Send tap a no-op --
 * verify the value and retype until it sticks.
 */
async function typePrompt(evalInApp, container, name, text) {
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      await container.type({ name, ref: COMPOSER_INPUT, text });
    } catch {
      await sleep(200);
      continue;
    }
    await sleep(250);
    const value = await evalInApp(
      `(() => (appDoc.querySelector(${JSON.stringify(COMPOSER_INPUT)}) || {}).value ?? '')()`,
    ).catch(() => '');
    if (typeof value === 'string' && value.includes(text)) return true;
  }
  fail(`could not type '${text}' into ${COMPOSER_INPUT}`);
  return false;
}

/**
 * The DebugFab (dev-tools badge) docks bottom-right by default -- the same
 * corner the composer's Send button ends up in, so the FAB sits on top of it
 * and the hit-tested tapInApp refuses. Dispatching the same pointer/mouse
 * sequence directly on the Send button sidesteps that dev-tooling overlap
 * (the established clickThroughDebugFab pattern) without touching app code.
 */
async function tapComposerSend(evalInApp) {
  const outcome = await evalInApp(`(() => {
    const el = appDoc.querySelector(${JSON.stringify(COMPOSER_SEND)});
    if (!el) return { ok: false, reason: 'no element matches ' + ${JSON.stringify(COMPOSER_SEND)} };
    const r = el.getBoundingClientRect();
    if (!r.width || !r.height) return { ok: false, reason: 'send button has no box' };
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const opts = { bubbles: true, cancelable: true, view: appDoc.defaultView, button: 0, clientX: x, clientY: y };
    el.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerdown', { ...opts, pointerId: 1, isPrimary: true }));
    el.dispatchEvent(new appDoc.defaultView.MouseEvent('mousedown', opts));
    el.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerup', { ...opts, pointerId: 1, isPrimary: true }));
    el.dispatchEvent(new appDoc.defaultView.MouseEvent('mouseup', opts));
    el.dispatchEvent(new appDoc.defaultView.MouseEvent('click', opts));
    return { ok: true };
  })()`);
  if (!outcome?.ok) {
    fail(`tapComposerSend: ${outcome?.reason ?? 'unknown failure'}`);
    return false;
  }
  return true;
}

try {
  await withServedApp(
    { port: 8209, name: 'coding-agent-plan-verify', path: 'coding-agent' },
    async ({ container, name, evalInApp, tapInApp }) => {
      // ---- Given: the list shows the fixture rows -----------------------
      let listReady = null;
      for (let attempt = 0; attempt < 16; attempt++) {
        await sleep(300);
        listReady = await evalInApp(`(() => ({
          list: !!appDoc.querySelector(${JSON.stringify(LIST)}),
          empty: !!appDoc.querySelector(${JSON.stringify(EMPTY)}),
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
            JSON.stringify(listReady),
        );
        return;
      }
      const planRow = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(PLAN_ROW)})`,
      );
      if (!planRow) {
        fail(`the plan fixture thread is missing from the list`);
        return;
      }

      // ---- When ThreadRow-fx-thread-fx-plan is tapped ---------------------
      // Then, within ~5s, the pathname ends with the thread route, ThreadScreen
      // has a non-zero box, and the fixture's proposed-plan card renders.
      const thread = await navigateIntoThread(
        evalInApp,
        tapInApp,
        PLAN_ROW,
        PLAN_PATH,
      );
      if (thread === null) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${PLAN_ROW} did not reach a rendered thread screen: ` +
            `pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }
      const fixturePlanCard = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(PLAN_CARD)})`,
      );
      if (!fixturePlanCard) {
        fail(`the fx-plan fixture's PlanCard is not rendered on ${PLAN_PATH}`);
        return;
      }
      const planThreadErrors = (await readPageErrors(evalInApp)).filter((e) =>
        FATAL_ERROR_RE.test(e),
      );
      if (planThreadErrors.length > 0) {
        fail(
          `the plan thread screen raised fatal page errors: ` +
            `${JSON.stringify(planThreadErrors)}`,
        );
        return;
      }

      // ---- (1) The scripted flow runs on a sendable thread ----------------
      // The fx-plan snapshot is `latestTurn.state: running`, which swaps the
      // composer's Send for Stop -- a running turn cannot carry a new prompt.
      // Drive the Plan-segment + 'fixture:plan' + Implement flow on
      // fx-thread-fx-basic whose latestTurn is `completed` (Send renders).
      const back = await backToList(evalInApp, tapInApp);
      if (back === null) {
        fail(
          `the header back control did not return to the list with its rows`,
        );
        return;
      }
      const scriptedThread = await navigateIntoThread(
        evalInApp,
        tapInApp,
        SCRIPTED_ROW,
        SCRIPTED_PATH,
      );
      if (scriptedThread === null) {
        fail(
          `tapping ${SCRIPTED_ROW} did not reach a rendered thread screen`,
        );
        return;
      }
      const sendPresent = await evalInApp(
        `(() => ({
          send: !!appDoc.querySelector(${JSON.stringify(COMPOSER_SEND)}),
          stop: !!appDoc.querySelector(${JSON.stringify(COMPOSER_STOP)}),
        }))()`,
      );
      if (!sendPresent?.send) {
        fail(
          `cannot send 'fixture:plan' on ${SCRIPTED_THREAD}: the composer ` +
            `renders ${JSON.stringify(COMPOSER_STOP)} (send=${sendPresent?.send}, ` +
            `stop=${sendPresent?.stop})`,
        );
        return;
      }

      // ---- (2) When the Plan segment is tapped ----------------------------
      // Then the composer placeholder flips from the default prompt to the
      // plan-mode prompt.
      await tapInApp(PLAN_SEGMENT);
      let placeholder = null;
      for (let attempt = 0; attempt < 10; attempt++) {
        placeholder = await evalInApp(
          `(() => (appDoc.querySelector(${JSON.stringify(COMPOSER_INPUT)}) || {}).placeholder ?? null)()`,
        ).catch(() => null);
        if (placeholder === PLAN_PLACEHOLDER) break;
        await sleep(300);
      }
      if (placeholder !== PLAN_PLACEHOLDER) {
        fail(
          `tapping ${PLAN_SEGMENT} did not change the composer placeholder to ` +
            `'${PLAN_PLACEHOLDER}' (saw ${JSON.stringify(placeholder)})`,
        );
        return;
      }

      // ---- (3) When 'fixture:plan' is sent --------------------------------
      // Then a PlanCard is visible on the feed.
      if (!(await typePrompt(evalInApp, container, name, 'fixture:plan'))) return;
      if (!(await tapComposerSend(evalInApp))) return;

      let planCardVisible = false;
      for (let attempt = 0; attempt < 12; attempt++) {
        planCardVisible = await evalInApp(
          `appDoc.querySelectorAll(${JSON.stringify(PLAN_CARD)}).length > 0`,
        ).catch(() => false);
        if (planCardVisible) break;
        await sleep(350);
      }
      if (!planCardVisible) {
        fail(`no PlanCard is visible after sending 'fixture:plan'`);
        return;
      }

      // ---- (4) When the PlanCard's Implement button is tapped -------------
      // Then a thread.turn.start carrying sourceProposedPlan reaches the wire
      // and its interactionMode is 'default' so the build turn runs even
      // though the composer is in Plan mode.
      const implementPresent = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(PLAN_IMPLEMENT)})`,
      );
      if (!implementPresent) {
        fail(`no ${PLAN_IMPLEMENT} button is rendered`);
        return;
      }
      await tapInApp(PLAN_IMPLEMENT);
      let implement = null;
      for (let attempt = 0; attempt < 8 && implement === null; attempt++) {
        let received = [];
        try {
          received = await fetchReceived();
        } catch (_err) {
          await sleep(500);
          continue;
        }
        implement =
          Array.isArray(received)
            ? received.find(
                (c) =>
                  c &&
                  c.type === 'thread.turn.start' &&
                  c.sourceProposedPlan !== undefined &&
                  c.sourceProposedPlan !== null &&
                  c.interactionMode === 'default',
              )
            : null;
        if (implement !== undefined && implement !== null) break;
        await sleep(500);
      }
      if (implement === null) {
        let received = [];
        try {
          received = await fetchReceived();
        } catch {
          // fall through with an empty array for the diagnostic message
        }
        fail(
          `tapping ${PLAN_IMPLEMENT} did not produce a thread.turn.start with ` +
            `sourceProposedPlan and interactionMode='default' on the wire within 4s: ` +
            `${JSON.stringify(received)}`,
        );
        return;
      }
      const planCardAfter = await evalInApp(
        `appDoc.querySelectorAll(${JSON.stringify(PLAN_CARD)}).length > 0`,
      ).catch(() => false);
      if (!planCardAfter) {
        fail(
          `tapping ${PLAN_IMPLEMENT} unmounted the PlanCard -- unexpected`,
        );
        return;
      }

      // ---- (5) No fatal page errors may survive the run ------------------
      const pageErrors = (await readPageErrors(evalInApp)).filter((e) =>
        FATAL_ERROR_RE.test(e),
      );
      if (pageErrors.length > 0) {
        fail(`fatal page errors at end of run: ${JSON.stringify(pageErrors)}`);
        return;
      }

      console.log(
        `ok: list rendered ${listReady.rowCount} fixture rows; tapping ` +
          `${PLAN_ROW} reached ${PLAN_PATH} with a non-zero ThreadScreen box and ` +
          `the fixture PlanCard; tapping ${PLAN_SEGMENT} on ${SCRIPTED_THREAD} ` +
          `(composer Send, since the ${PLAN_THREAD} snapshot is running) flipped ` +
          `the composer placeholder to '${PLAN_PLACEHOLDER}'; sending ` +
          `'fixture:plan' surfaced a PlanCard; ${PLAN_IMPLEMENT} dispatched a ` +
          `thread.turn.start carrying sourceProposedPlan with interactionMode ` +
          `'default' on the wire (the PlanCard stays mounted); no ` +
          `Minified-React-error/isReady page errors.`,
      );
    },
  );
} finally {
  await server.stop();
}