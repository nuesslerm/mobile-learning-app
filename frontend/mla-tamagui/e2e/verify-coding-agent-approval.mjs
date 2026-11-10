/**
 * Phase-9 acceptance (#252, #254) as amended by the tranche-4 honesty fixes
 * (`docs/archive/plans/coding-agent-frontend-fixes/contract.md`): the thread route is
 * actually reached, tapping Allow on the pending command approval records a
 * `thread.approval.respond` accept on the wire, and sending `fixture:approval`
 * opens a fresh approval that a Deny tap resolves to decline. The scripted
 * send runs on `fx-thread-fx-basic` (latestTurn `completed`, so the composer
 * renders Send) because the fx-approval fixture snapshot is
 * `latestTurn.state: running`, which swaps Send for Stop -- a running turn
 * cannot carry a new prompt. Every Given/When/Then named here is executed and
 * asserted with `fail(...)` on miss; a `__pageErrors` entry containing
 * `Minified React error` or `isReady` fails the check.
 *
 * The WebSocket is the fake agent server in
 * `frontend/mla-tamagui/features/coding-agent/src/testing/node/fakeWsAgentServer.mjs` on port 8787; the
 * fx-approval fixture thread (`fx-thread-fx-approval`) carries two open
 * `request.opened` activities, and `/__received` records every command the
 * client dispatches.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const LIST = '[data-testid="ThreadList"]';
const EMPTY = '[data-testid="ThreadListEmpty"]';
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const APPROVAL_THREAD = 'fx-thread-fx-approval';
const APPROVAL_ROW = `[data-testid="ThreadRow-${APPROVAL_THREAD}"]`;
const APPROVAL_PATH = `/coding-agent/${APPROVAL_THREAD}`;
const COMMAND_REQUEST = 'fx-req-approval-command';
const COMMAND_CARD = `[data-testid="ApprovalCard-${COMMAND_REQUEST}"]`;
const COMMAND_ALLOW = `[data-testid="ApprovalCardAllow-${COMMAND_REQUEST}"]`;
const SCRIPTED_THREAD = 'fx-thread-fx-basic';
const SCRIPTED_ROW = `[data-testid="ThreadRow-${SCRIPTED_THREAD}"]`;
const SCRIPTED_PATH = `/coding-agent/${SCRIPTED_THREAD}`;
const SCRIPTED_REQUEST_PREFIX = `fx-req-${SCRIPTED_THREAD}-approval`;
const COMPOSER_INPUT = '[data-testid="ThreadComposerInput"]';
const COMPOSER_SEND = '[data-testid="ThreadComposerSendButton"]';
const COMPOSER_STOP = '[data-testid="ThreadComposerStopButton"]';
const BACK = '[data-testid="ThreadScreenHeaderBack"]';
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

async function approvalResponds(evalInApp, requestId, decision, sinceCount) {
  let received = [];
  try {
    received = await fetchReceived();
  } catch (err) {
    fail(`/__received could not be read: ${err?.message ?? err}`);
    return false;
  }
  return received.slice(sinceCount).some(
    (c) =>
      c &&
      c.type === 'thread.approval.respond' &&
      c.requestId === requestId &&
      c.decision === decision,
  );
}

try {
  await withServedApp(
    { port: 8207, name: 'coding-agent-approval-verify', path: 'coding-agent' },
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
      const approvalRow = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(APPROVAL_ROW)})`,
      );
      if (!approvalRow) {
        fail(`the approval fixture thread is missing from the list`);
        return;
      }

      // ---- When ThreadRow-fx-thread-fx-approval is tapped -----------------
      // Then, within ~5s, the pathname ends with the thread route, ThreadScreen
      // has a non-zero box, and the pending command approval card renders.
      const thread = await navigateIntoThread(
        evalInApp,
        tapInApp,
        APPROVAL_ROW,
        APPROVAL_PATH,
      );
      if (thread === null) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${APPROVAL_ROW} did not reach a rendered thread screen: ` +
            `pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }
      const commandCard = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(COMMAND_CARD)})`,
      );
      if (!commandCard) {
        fail(
          `the pending command approval card ${COMMAND_CARD} is not rendered`,
        );
        return;
      }

      // ---- (1) When the pending command approval's Allow once is tapped ----
      // Then /__received gains a thread.approval.respond carrying that
      // requestId with decision accept.
      const beforeAllow = (await fetchReceived()).length;
      await tapInApp(COMMAND_ALLOW);
      let accepted = false;
      for (let attempt = 0; attempt < 12; attempt++) {
        accepted = await approvalResponds(
          evalInApp,
          COMMAND_REQUEST,
          'accept',
          beforeAllow,
        );
        if (accepted) break;
        await sleep(350);
      }
      if (!accepted) {
        const received = await fetchReceived().catch(() => []);
        fail(
          `tapping ${COMMAND_ALLOW} did not put a thread.approval.respond ` +
            `(requestId=${COMMAND_REQUEST}, decision=accept) on the wire: ` +
            JSON.stringify(received),
        );
        return;
      }
      // The next step's back-navigation is a full page load that discards the
      // current document's error log, so the fixture-thread mount errors are
      // checked here, while they are still in the same document.
      const approvalThreadErrors = (await readPageErrors(evalInApp)).filter((e) =>
        FATAL_ERROR_RE.test(e),
      );
      if (approvalThreadErrors.length > 0) {
        fail(
          `the approval thread screen raised fatal page errors: ` +
            `${JSON.stringify(approvalThreadErrors)}`,
        );
        return;
      }

      // ---- (2) When 'fixture:approval' is sent ----------------------------
      // The fx-approval snapshot is `latestTurn.state: running`, which swaps
      // the composer's Send for Stop -- a running turn cannot carry a new
      // prompt. Drive the scripted prompt on fx-thread-fx-basic instead,
      // whose latestTurn is `completed`, so the composer renders Send.
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
          `cannot send 'fixture:approval' on ${SCRIPTED_THREAD}: the composer ` +
            `renders ${JSON.stringify(COMPOSER_STOP)} (send=${sendPresent?.send}, ` +
            `stop=${sendPresent?.stop})`,
        );
        return;
      }
      if (!(await typePrompt(evalInApp, container, name, 'fixture:approval'))) return;
      if (!(await tapComposerSend(evalInApp))) return;

      // Then a new ApprovalCard opens for the scripted request id, and when its
      // Deny is tapped the wire records decision decline for that requestId.
      let scriptedCard = null;
      for (let attempt = 0; attempt < 12; attempt++) {
        scriptedCard = await evalInApp(`(() => {
          const ids = [...appDoc.querySelectorAll('[data-testid^="ApprovalCard-"]')]
            .map((e) => e.getAttribute('data-testid').replace(/^ApprovalCard-/, ''));
          return ids.find((id) => id.startsWith(${JSON.stringify(SCRIPTED_REQUEST_PREFIX)})) ?? null;
        })()`).catch(() => null);
        if (scriptedCard) break;
        await sleep(350);
      }
      if (!scriptedCard) {
        fail(
          `sending 'fixture:approval' did not open a fresh approval card for ` +
            `a requestId under ${SCRIPTED_REQUEST_PREFIX}`,
        );
        return;
      }
      const beforeDeny = (await fetchReceived()).length;
      await tapInApp(`[data-testid="ApprovalCardDeny-${scriptedCard}"]`);
      let declined = false;
      for (let attempt = 0; attempt < 12; attempt++) {
        declined = await approvalResponds(
          evalInApp,
          scriptedCard,
          'decline',
          beforeDeny,
        );
        if (declined) break;
        await sleep(350);
      }
      if (!declined) {
        fail(
          `tapping Deny on ${scriptedCard} did not record decision decline: ` +
            JSON.stringify(await fetchReceived().catch(() => [])),
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
          `${APPROVAL_ROW} reached ${APPROVAL_PATH} with a non-zero ThreadScreen ` +
          `box and the ${COMMAND_REQUEST} approval card; tapping ${COMMAND_ALLOW} ` +
          `recorded thread.approval.respond(decision=accept) for ` +
          `${COMMAND_REQUEST}; sending 'fixture:approval' on ${SCRIPTED_THREAD} ` +
          `(composer Send, since the ${APPROVAL_THREAD} snapshot is running) ` +
          `opened ${scriptedCard} and its Deny recorded decision=decline; no ` +
          `Minified-React-error/isReady page errors.`,
      );
    },
  );
} finally {
  await server.stop();
}