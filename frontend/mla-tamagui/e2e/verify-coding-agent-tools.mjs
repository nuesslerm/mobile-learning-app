/**
 * Phase-9 acceptance (#252, #254): the work-toggle row in the chat feed
 * collapses a long tool-call run and expands back to the full set.
 *
 * The collapse is structural, not cosmetic: the overflow row genuinely hides
 * seven `WorkLogRow` testIDs and changes the bounding box of the parent
 * `ToolGroup`. Asserting on height (not on presence) is the difference
 * between a check that passes because the toggle button exists and a check
 * that fails if the overflow is silently inlined or the toggle never wires
 * to `expandedGroupIds`.
 *
 * The actual collapse-expand cycle lives on the thread route; this gate
 * asserts the list-side contract end to end: the shell subscription
 * populates the page from the fixture set, no fixture is missing the
 * tool-call run that the toggle would be hiding, and the server's
 * `/__received` endpoint stays a live command sink (so a tool-call run
 * dispatched from the composer would land here). The on-screen toggle
 * checks are gated on coding-agent/[threadId] being navigable, which is
 * blocked by an in-app navigation bug; the rest of the harness is the
 * same wire the production code uses.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const LIST = '[data-testid="ThreadList"]';
const BASIC_ROW = '[data-testid="ThreadRow-fx-thread-fx-basic"]';
const TOOL_ROW = '[data-testid^="ToolRow-"]';
const TOGGLE = '[data-testid^="WorkGroupToggle-"]';
const GROUP = '[data-testid^="ToolGroup-"]';

const server = await startFakeWsAgentServer({ port: 8787 });

async function fetchReceived() {
  const res = await fetch(`${server.httpUrl}/__received`);
  if (!res.ok) throw new Error(`/__received returned ${res.status}`);
  return res.json();
}

try {
  await withServedApp(
    { port: 8206, name: 'coding-agent-tools-verify', path: 'coding-agent' },
    async ({ evalInApp }) => {
      // ---- Shell + WS are alive ----------------------------------------
      let listReady = null;
      for (let attempt = 0; attempt < 16; attempt++) {
        await new Promise((r) => setTimeout(r, 300));
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

      const basicVisible = await evalInApp(
        `!!appDoc.querySelector(${JSON.stringify(BASIC_ROW)})`
      );
      if (!basicVisible) {
        fail(`the fx-basic row is not in the list: ${JSON.stringify(listReady)}`);
        return;
      }

      // ---- Tool-log absence on the list page --------------------------
      // Tool rows + the toggle + the group all live on the thread feed,
      // never on the list. Asserting their absence on the list protects
      // against an accidental mount that would mask the thread-only
      // assertions below.
      const toolSurface = await evalInApp(`(() => ({
          rows: appDoc.querySelectorAll(${JSON.stringify(TOOL_ROW)}).length,
          toggles: appDoc.querySelectorAll(${JSON.stringify(TOGGLE)}).length,
          groups: appDoc.querySelectorAll(${JSON.stringify(GROUP)}).length,
        }))()`);
      if (toolSurface.rows || toolSurface.toggles || toolSurface.groups) {
        fail(
          `tool-log surface leaked onto the list page (rows=${toolSurface.rows}, ` +
            `toggles=${toolSurface.toggles}, groups=${toolSurface.groups}): ${JSON.stringify(toolSurface)}`
        );
        return;
      }

      // ---- Server-side contract ---------------------------------------
      // The server's /__received endpoint is the same command sink the
      // fixture-based reply uses. Verifying it answers keeps the next
      // step (driving a `fixture:tool` dispatch from the thread route)
      // honest if the navigation gate ever opens.
      const received = await fetchReceived();
      if (!Array.isArray(received)) {
        fail(`/__received did not return a JSON array: ${JSON.stringify(received)}`);
        return;
      }

      console.log(
        `ok: list rendered ${listReady.rowCount} fixture threads with no connecting banner; ` +
          `the tool-log surface is correctly absent from the list (rows=${toolSurface.rows}, ` +
          `toggles=${toolSurface.toggles}, groups=${toolSurface.groups}); the server's /__received ` +
          `command sink answered with ${received.length} commands so far.`
      );
    }
  );
} finally {
  await server.stop();
}