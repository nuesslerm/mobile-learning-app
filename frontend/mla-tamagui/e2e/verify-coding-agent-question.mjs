/**
 * Phase-9 acceptance (#252, #254) as amended by the tranche-4 honesty fixes
 * (`docs/archive/plans/coding-agent-frontend-fixes/contract.md`): the thread route is
 * actually reached, tapping option index 1 and Submit dispatches a
 * `thread.user-input.respond` that carries that option's label, and the
 * scripted resume message reaches the feed. The scripted `fixture:question`
 * send runs on `fx-thread-fx-basic` (latestTurn `completed`, so the composer
 * renders Send) because the fx-user-input fixture snapshot is
 * `latestTurn.state: running`, which swaps Send for Stop -- a running turn
 * cannot carry a new prompt, and the fake server only emits the resume
 * message from a paused `fixture:question` script. Every Given/When/Then
 * named here is executed and asserted with `fail(...)` on miss; a
 * `__pageErrors` entry containing `Minified React error` or `isReady` fails
 * the check.
 *
 * The WebSocket is the fake agent server in
 * `frontend/mla-tamagui/features/coding-agent/src/testing/node/fakeWsAgentServer.mjs` on port 8787; the
 * fx-user-input fixture thread (`fx-thread-fx-user-input`) carries an open
 * `user-input.requested` activity with two questions, and `/__received`
 * records every command the client dispatches.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const LIST = '[data-testid="ThreadList"]';
const EMPTY = '[data-testid="ThreadListEmpty"]';
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const USER_INPUT_THREAD = 'fx-thread-fx-user-input';
const USER_INPUT_ROW = `[data-testid="ThreadRow-${USER_INPUT_THREAD}"]`;
const USER_INPUT_PATH = `/coding-agent/${USER_INPUT_THREAD}`;
const USER_REQUEST = 'fx-req-user-input';
const USER_INPUT_CARD = `[data-testid="UserInputCard-${USER_REQUEST}"]`;
const USER_INPUT_OPTION = `[data-testid^="UserInputCardOption-${USER_REQUEST}-"]`;
const USER_INPUT_SUBMIT = `[data-testid="UserInputCardSubmit-${USER_REQUEST}"]`;
const THREAD_FEED = '[data-testid="ThreadFeed"]';
const SCRIPTED_THREAD = 'fx-thread-fx-basic';
const SCRIPTED_ROW = `[data-testid="ThreadRow-${SCRIPTED_THREAD}"]`;
const SCRIPTED_PATH = `/coding-agent/${SCRIPTED_THREAD}`;
const SCRIPTED_REQUEST = `fx-req-${SCRIPTED_THREAD}-user-input`;
const COMPOSER_INPUT = '[data-testid="ThreadComposerInput"]';
const COMPOSER_SEND = '[data-testid="ThreadComposerSendButton"]';
const COMPOSER_STOP = '[data-testid="ThreadComposerStopButton"]';
const BACK = '[data-testid="ThreadScreenHeaderBack"]';
const RESUME_TEXT = 'Got it. Continuing with your answers.';
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
 * Taps the option at `index` among all rendered options for USER_REQUEST.
 * The impl gives q-auth's options indices 0..1 and q-scopes' options indices
 * 0..2, so the DOM order is [OAuth, API key, read, write, admin]: index 1 is
 * the 'API key' option whose label the respond must carry, and a scopes option
 * (e.g. index 2, 'read') must also be selected because the card disables
 * Submit until every question has an answer. Mirrors tapInApp's hit-tested
 * pointer/mouse dispatch.
 */
async function tapCardOptionAt(evalInApp, index) {
  const outcome = await evalInApp(`(() => {
    const els = appDoc.querySelectorAll(${JSON.stringify(USER_INPUT_OPTION)});
    const el = els[${index}];
    if (!el) return { ok: false, reason: 'no option at index ' + ${index} };
    const r = el.getBoundingClientRect();
    if (!r.width || !r.height) return { ok: false, reason: 'option has no box' };
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const hit = appDoc.elementFromPoint(x, y);
    if (!hit || !(el === hit || el.contains(hit))) {
      return { ok: false, reason: 'something else is on top at the option centre' };
    }
    const opts = { bubbles: true, cancelable: true, view: appDoc.defaultView, button: 0, clientX: x, clientY: y };
    hit.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerdown', { ...opts, pointerId: 1, isPrimary: true }));
    hit.dispatchEvent(new appDoc.defaultView.MouseEvent('mousedown', opts));
    hit.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerup', { ...opts, pointerId: 1, isPrimary: true }));
    hit.dispatchEvent(new appDoc.defaultView.MouseEvent('mouseup', opts));
    hit.dispatchEvent(new appDoc.defaultView.MouseEvent('click', opts));
    return { ok: true };
  })()`);
  if (!outcome?.ok) {
    fail(`tapCardOptionAt(${index}): ${outcome?.reason ?? 'unknown failure'}`);
    return false;
  }
  return true;
}

try {
  await withServedApp(
    { port: 8208, name: 'coding-agent-question-verify', path: 'coding-agent' },
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
      const userInputRow = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(USER_INPUT_ROW)})`,
      );
      if (!userInputRow) {
        fail(`the user-input fixture thread is missing from the list`);
        return;
      }

      // ---- When ThreadRow-fx-thread-fx-user-input is tapped ---------------
      // Then, within ~5s, the pathname ends with the thread route, ThreadScreen
      // has a non-zero box, and the pending user-input card renders.
      const thread = await navigateIntoThread(
        evalInApp,
        tapInApp,
        USER_INPUT_ROW,
        USER_INPUT_PATH,
      );
      if (thread === null) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${USER_INPUT_ROW} did not reach a rendered thread screen: ` +
            `pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }
      const cardPresent = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(USER_INPUT_CARD)})`,
      );
      if (!cardPresent) {
        fail(`the pending user-input card ${USER_INPUT_CARD} is not rendered`);
        return;
      }

      // ---- (1) When option index 1 + Submit are tapped --------------------
      // A scopes option is also tapped so Submit stops being disabled (the card
      // requires an answer for every question). Then /__received gains a
      // thread.user-input.respond whose answers carry the label of option index
      // 1 ('API key') for question q-auth.
      if (!(await tapCardOptionAt(evalInApp, 1))) return;
      if (!(await tapCardOptionAt(evalInApp, 2))) return;
      await sleep(250);
      await tapInApp(USER_INPUT_SUBMIT);

      let responded = null;
      for (let attempt = 0; attempt < 12; attempt++) {
        const received = await fetchReceived().catch(() => []);
        responded =
          received.find(
            (c) =>
              c &&
              c.type === 'thread.user-input.respond' &&
              c.requestId === USER_REQUEST,
          ) ?? null;
        if (responded) break;
        await sleep(350);
      }
      const qAuthLabel = responded?.answers?.['q-auth'];
      if (!responded || qAuthLabel !== 'API key') {
        fail(
          `Submit did not put a thread.user-input.respond on the wire whose ` +
            `answers carry the 'API key' label for q-auth: ` +
            JSON.stringify(responded ?? (await fetchReceived().catch(() => []))),
        );
        return;
      }
      // The next step's back-navigation is a full page load that discards the
      // current document's error log, so the fixture-thread mount errors are
      // checked here, while they are still in the same document.
      const questionThreadErrors = (await readPageErrors(evalInApp)).filter((e) =>
        FATAL_ERROR_RE.test(e),
      );
      if (questionThreadErrors.length > 0) {
        fail(
          `the user-input thread screen raised fatal page errors: ` +
            `${JSON.stringify(questionThreadErrors)}`,
        );
        return;
      }

      // ---- (2) Then the scripted resume message reaches the feed ----------
      // The fake server only emits 'Got it. Continuing with your answers.' from
      // continueScriptAfterInterrupt, which runs only when a 'fixture:question'
      // script is paused -- and that script needs a composer send. The
      // fx-user-input snapshot is `latestTurn.state: running` (composer renders
      // Stop), so the scripted prompt is driven on fx-thread-fx-basic whose
      // latestTurn is `completed` (composer renders Send).
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
          `cannot send 'fixture:question' on ${SCRIPTED_THREAD}: the composer ` +
            `renders ${JSON.stringify(COMPOSER_STOP)} (send=${sendPresent?.send}, ` +
            `stop=${sendPresent?.stop})`,
        );
        return;
      }
      if (!(await typePrompt(evalInApp, container, name, 'fixture:question'))) return;
      if (!(await tapComposerSend(evalInApp))) return;

      let scriptedCard = null;
      for (let attempt = 0; attempt < 12; attempt++) {
        scriptedCard = await evalInApp(`(() => {
          const el = appDoc.querySelector(${JSON.stringify(`[data-testid="UserInputCard-${SCRIPTED_REQUEST}"]`)});
          return el ? ${JSON.stringify(SCRIPTED_REQUEST)} : null;
        })()`).catch(() => null);
        if (scriptedCard) break;
        await sleep(350);
      }
      if (!scriptedCard) {
        fail(
          `sending 'fixture:question' did not open a user-input card for ` +
            `${SCRIPTED_REQUEST}`,
        );
        return;
      }
      const scriptedOptionOutcome = await evalInApp(`(() => {
        const el = appDoc.querySelector(${JSON.stringify(`[data-testid="UserInputCardOption-${SCRIPTED_REQUEST}-0"]`)});
        if (!el) return { ok: false, reason: 'no option 0' };
        const r = el.getBoundingClientRect();
        if (!r.width || !r.height) return { ok: false, reason: 'option has no box' };
        const x = r.left + r.width / 2;
        const y = r.top + r.height / 2;
        const hit = appDoc.elementFromPoint(x, y);
        if (!hit || !(el === hit || el.contains(hit))) {
          return { ok: false, reason: 'something else is on top at the option centre' };
        }
        const opts = { bubbles: true, cancelable: true, view: appDoc.defaultView, button: 0, clientX: x, clientY: y };
        hit.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerdown', { ...opts, pointerId: 1, isPrimary: true }));
        hit.dispatchEvent(new appDoc.defaultView.MouseEvent('mousedown', opts));
        hit.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerup', { ...opts, pointerId: 1, isPrimary: true }));
        hit.dispatchEvent(new appDoc.defaultView.MouseEvent('mouseup', opts));
        hit.dispatchEvent(new appDoc.defaultView.MouseEvent('click', opts));
        return { ok: true };
      })()`);
      if (!scriptedOptionOutcome?.ok) {
        fail(
          `selecting option 0 on ${SCRIPTED_REQUEST}: ` +
            `${scriptedOptionOutcome?.reason ?? 'unknown failure'}`,
        );
        return;
      }
      await sleep(250);
      await tapInApp(`[data-testid="UserInputCardSubmit-${SCRIPTED_REQUEST}"]`);
      await sleep(300);

      let resumed = false;
      for (let attempt = 0; attempt < 12; attempt++) {
        const feed = await evalInApp(
          `(() => {
            const el = appDoc.querySelector(${JSON.stringify(THREAD_FEED)});
            return el ? el.textContent : '';
          })()`,
        ).catch(() => '');
        if (typeof feed === 'string' && feed.includes(RESUME_TEXT)) {
          resumed = true;
          break;
        }
        await sleep(350);
      }
      if (!resumed) {
        fail(
          `after submitting the scripted 'fixture:question' input on ` +
            `${SCRIPTED_THREAD}, the feed never gained the scripted resume ` +
            `message '${RESUME_TEXT}'`,
        );
        return;
      }

      // ---- (3) No fatal page errors may survive the run ------------------
      const pageErrors = (await readPageErrors(evalInApp)).filter((e) =>
        FATAL_ERROR_RE.test(e),
      );
      if (pageErrors.length > 0) {
        fail(`fatal page errors at end of run: ${JSON.stringify(pageErrors)}`);
        return;
      }

      console.log(
        `ok: list rendered ${listReady.rowCount} fixture rows; tapping ` +
          `${USER_INPUT_ROW} reached ${USER_INPUT_PATH} with a non-zero ` +
          `ThreadScreen box and the ${USER_REQUEST} card; tapping option index 1 ` +
          `('API key') + Submit recorded thread.user-input.respond with ` +
          `answers=${JSON.stringify(responded.answers)}; sending 'fixture:question' ` +
          `on ${SCRIPTED_THREAD} (composer Send, since the ${USER_INPUT_THREAD} ` +
          `snapshot is running) opened ${SCRIPTED_REQUEST}, its Submit put a ` +
          `respond on the wire, and the scripted resume message '${RESUME_TEXT}' ` +
          `reached the feed; no Minified-React-error/isReady page errors.`,
      );
    },
  );
} finally {
  await server.stop();
}