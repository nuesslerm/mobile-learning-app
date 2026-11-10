/**
 * d12-e2e-runs-transcript: prove the dispatch-run detail screen renders the
 * finished fixture as a read-only transcript (no composer, presence of the
 * dashboard link, the header's tokens + cost), and that the missing-run
 * route falls through to a visible error.
 *
 * Boots startFakeWsAgentServer on 8787 (so the FlatList populates with
 * threads and the DispatchRunsSection footer mounts) and
 * startFakeOpencodeLogServer on 8091 (the HTTP gateway baked via
 * EXPO_PUBLIC_VM_OPS_API_BASE_URL). Opens /coding-agent, taps the
 * finished row to navigate client-side into the run detail screen
 * (the static export does not serve a parameterized URL for /[runId],
 * so direct visits 404 -- routing through the in-app router avoids
 * that), polls for the feed, asserts the finished fixture's prompt
 * excerpt, first assistant text and the three tool names appear in
 * the feed text, that the feed box is non-trivial and the header
 * carries the tokens/cost, that no composer testID is mounted, that
 * the dashboard link button is wide enough to be tappable, then
 * reprograms history to /coding-agent/runs/does-not-exist and pops
 * the router state so it falls through to the error testID. Exits 1
 * on any failure with a FAIL: line.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';
import { startFakeOpencodeLogServer } from '../features/coding-agent/src/testing/node/fakeOpencodeLogServer.mjs';

const DISPATCH_RUNS_SECTION = '[data-testid="DispatchRunsSection"]';
const FINISHED_ROW = '[data-testid="DispatchRunRow-fx-run-finished"]';
const DISPATCH_RUN_SCREEN = '[data-testid="DispatchRunScreen"]';
const DISPATCH_RUN_HEADER = '[data-testid="DispatchRunHeader"]';
const DISPATCH_RUN_FEED = '[data-testid="DispatchRunFeed"]';
const DISPATCH_RUN_DASHBOARD_LINK = '[data-testid="DispatchRunDashboardLink"]';
const DISPATCH_RUN_ERROR = '[data-testid="DispatchRunError"]';
// The dispatch-run screen has no composer; the ThreadScreen does. Checking
// either the composer's outer wrapper or the send-button testID catches a
// regression that accidentally mounts ThreadComposer on the run detail.
const COMPOSER_WRAPPER = '[data-testid="ThreadComposer"]';
const COMPOSER_SEND = '[data-testid="ThreadComposerSendButton"]';

const FATAL_ERROR_RE = /Minified React error|isReady/;

const FINISHED_PROMPT_EXCERPT = 'Add the DispatchRunsSection';
const FINISHED_FIRST_ASSISTANT_TEXT = 'Adding the section now.';
const FINISHED_TOOL_NAMES = ['read', 'edit', 'bash'];
const FINISHED_TOKENS_TEXT = '12345';
const FINISHED_COST_TEXT = '$0.0123';
const FEED_MIN_HEIGHT_PX = 200;
const DASHBOARD_LINK_MIN_WIDTH_PX = 80;
const MISSING_RUN_ID = 'does-not-exist';

const wsServer = await startFakeWsAgentServer({ port: 8787 });
const logServer = await startFakeOpencodeLogServer({ port: 8091 });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function fetchReceived(server) {
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
    { port: 8206, name: 'coding-agent-runs-transcript-verify', path: 'coding-agent' },
    async ({ evalInApp, tapInApp }) => {
      // ---- Given: DispatchRunsSection mounted and the finished row is in view
      // -- FlatList only mounts its footer once it has data; this also waits
      // implicitly for the WS handshake to populate threads.
      let sectionView = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        await sleep(500);
        sectionView = await evalInApp(`(() => {
          const section = appDoc.querySelector(${JSON.stringify(DISPATCH_RUNS_SECTION)});
          const finished = appDoc.querySelector(${JSON.stringify(FINISHED_ROW)});
          const finishedRect = finished ? finished.getBoundingClientRect() : null;
          return {
            sectionPresent: !!section,
            finishedHeight: finishedRect ? finishedRect.height : 0,
            finishedTop: finishedRect ? finishedRect.top : 0,
            finishedBottom: finishedRect ? finishedRect.bottom : 0,
            vh: appDoc.defaultView.innerHeight,
          };
        })()`).catch(() => null);
        if (sectionView?.sectionPresent && sectionView.finishedHeight > 0) break;
      }
      if (!sectionView?.sectionPresent || sectionView.finishedHeight <= 0) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `DispatchRunsSection / DispatchRunRow-fx-run-finished never appeared: ` +
            `${JSON.stringify(sectionView)} pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }

      // ---- When the finished row is tapped -------------------------------
      // Scroll it into view first (the FlatList scroller on iphone-15-pro
      // pushes it under the keyboard area), then tap. expo-router transitions
      // client-side to /coding-agent/runs/fx-run-finished, which the static
      // export does not serve directly but the in-app router renders fine.
      await evalInApp(`(() => {
        const el = appDoc.querySelector(${JSON.stringify(FINISHED_ROW)});
        if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center' });
        return !!el;
      })()`).catch(() => null);
      await sleep(300);
      await tapInApp(FINISHED_ROW);

      // ---- Then DispatchRunScreen mounts and the feed renders ------------
      // The store fetches the fixture via the HTTP gateway on mount; poll for
      // the screen and the feed both present, the header carrying the finished
      // fixture's tokens text, and the feed box > 200 px -- the feed is
      // read-only once those are true.
      let transcriptView = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        await sleep(500);
        transcriptView = await evalInApp(`(() => {
          const screen = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_SCREEN)});
          const header = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_HEADER)});
          const feed = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_FEED)});
          const dashboardLink = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_DASHBOARD_LINK)});
          const composer = appDoc.querySelector(${JSON.stringify(COMPOSER_WRAPPER)});
          const composerSend = appDoc.querySelector(${JSON.stringify(COMPOSER_SEND)});
          const feedRect = feed ? feed.getBoundingClientRect() : null;
          const linkRect = dashboardLink ? dashboardLink.getBoundingClientRect() : null;
          return {
            path: appDoc.location.pathname,
            screenPresent: !!screen,
            headerPresent: !!header,
            feedPresent: !!feed,
            feedHeight: feedRect ? feedRect.height : 0,
            feedText: feed ? feed.textContent : '',
            headerText: header ? header.textContent : '',
            dashboardLinkPresent: !!dashboardLink,
            dashboardLinkWidth: linkRect ? linkRect.width : 0,
            composerPresent: !!composer,
            composerSendPresent: !!composerSend,
          };
        })()`).catch(() => null);
        if (
          transcriptView?.path.endsWith('/coding-agent/runs/fx-run-finished') &&
          transcriptView.screenPresent &&
          transcriptView.feedPresent &&
          transcriptView.feedHeight > FEED_MIN_HEIGHT_PX &&
          transcriptView.dashboardLinkPresent &&
          transcriptView.headerText.includes(FINISHED_TOKENS_TEXT)
        ) {
          break;
        }
      }

      if (!transcriptView?.screenPresent || !transcriptView.feedPresent) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${FINISHED_ROW} did not mount DispatchRunScreen + DispatchRunFeed: ` +
            `${JSON.stringify(transcriptView)} pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }
      if (transcriptView.feedHeight <= FEED_MIN_HEIGHT_PX) {
        fail(
          `DispatchRunFeed is shorter than expected (height=${transcriptView.feedHeight}px, ` +
            `want > ${FEED_MIN_HEIGHT_PX}px): ${JSON.stringify(transcriptView)}`,
        );
        return;
      }

      const feedText = transcriptView.feedText;
      const missingAssertions = [];
      if (!feedText.includes(FINISHED_PROMPT_EXCERPT)) {
        missingAssertions.push(`feed text does not contain prompt excerpt '${FINISHED_PROMPT_EXCERPT}'`);
      }
      if (!feedText.includes(FINISHED_FIRST_ASSISTANT_TEXT)) {
        missingAssertions.push(
          `feed text does not contain first assistant text '${FINISHED_FIRST_ASSISTANT_TEXT}'`,
        );
      }
      for (const toolName of FINISHED_TOOL_NAMES) {
        if (!feedText.includes(toolName)) {
          missingAssertions.push(`feed text does not contain tool name '${toolName}'`);
        }
      }
      if (missingAssertions.length > 0) {
        fail(
          `feed text assertions failed: ${missingAssertions.join('; ')} ` +
            `feedText=${JSON.stringify(feedText.slice(0, 600))}`,
        );
        return;
      }

      const headerText = transcriptView.headerText;
      if (!headerText.includes(FINISHED_TOKENS_TEXT)) {
        fail(
          `${DISPATCH_RUN_HEADER} text does not contain '${FINISHED_TOKENS_TEXT}' (tokens): ` +
            `${JSON.stringify(headerText)}`,
        );
        return;
      }
      if (!headerText.includes(FINISHED_COST_TEXT)) {
        fail(
          `${DISPATCH_RUN_HEADER} text does not contain '${FINISHED_COST_TEXT}' (cost): ` +
            `${JSON.stringify(headerText)}`,
        );
        return;
      }

      if (transcriptView.composerPresent) {
        fail(
          `ThreadComposer (testID 'Composer') unexpectedly mounted on the dispatch-run ` +
            `detail screen: ${JSON.stringify(transcriptView)}`,
        );
        return;
      }
      if (transcriptView.composerSendPresent) {
        fail(
          `composer send button (testID 'ThreadComposerSendButton') unexpectedly mounted on the ` +
            `dispatch-run detail screen: ${JSON.stringify(transcriptView)}`,
        );
        return;
      }

      if (transcriptView.dashboardLinkWidth <= DASHBOARD_LINK_MIN_WIDTH_PX) {
        fail(
          `DispatchRunDashboardLink is too narrow (width=${transcriptView.dashboardLinkWidth}px, ` +
            `want > ${DASHBOARD_LINK_MIN_WIDTH_PX}px): ${JSON.stringify(transcriptView)}`,
        );
        return;
      }

      // ---- (1) When the dashboard link is tapped --------------------------
      // Then the gateway records a GET on /opencode-log/api/runs/fx-run-finished
      // and the run detail screen is otherwise unchanged (it stays read-only;
      // Linking.openURL never lands inside the iframe).
      await evalInApp(`(() => {
        const el = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_DASHBOARD_LINK)});
        if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center' });
        return !!el;
      })()`).catch(() => null);
      await sleep(300);
      await tapInApp(DISPATCH_RUN_DASHBOARD_LINK);

      // ---- (2) Then the opencode-log fake recorded a detail GET ----------
      let received = [];
      try {
        received = await fetchReceived(logServer);
      } catch (err) {
        fail(
          `/__received could not be read from opencode-log server: ${err?.message ?? err}`,
        );
        return;
      }
      const detailCalls = received.filter(
        (r) =>
          r &&
          r.method === 'GET' &&
          typeof r.url === 'string' &&
          r.url.endsWith('/opencode-log/api/runs/fx-run-finished'),
      );
      if (detailCalls.length === 0) {
        fail(
          `expected at least one GET /opencode-log/api/runs/fx-run-finished on the wire, ` +
            `saw: ${JSON.stringify(received)}`,
        );
        return;
      }

      // ---- (3) When navigating to a run the fake does not know about ----
      // Then DispatchRunError appears with non-zero geometry, and the feed
      // is gone. The store's loadRun sets status 'error' + error message on
      // the gateway's 404, and the screen renders the error block.
      // Push the new URL through the same history mechanism expo-router
      // listens on -- the static export does not serve /[runId] directly,
      // so a full page reload would 404 before the route ever resolved.
      await evalInApp(`(() => {
        const newPath = ${JSON.stringify(`/mla-tamagui/coding-agent/runs/${MISSING_RUN_ID}`)};
        window.history.pushState({}, '', newPath);
        window.dispatchEvent(new PopStateEvent('popstate'));
        return window.location.pathname;
      })()`).catch(() => null);
      let errorView = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        await sleep(400);
        errorView = await evalInApp(`(() => {
          const error = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_ERROR)});
          const r = error ? error.getBoundingClientRect() : null;
          const feed = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_FEED)});
          return {
            path: appDoc.location.pathname,
            errorPresent: !!error,
            errorRect: r ? { width: r.width, height: r.height } : null,
            errorText: error ? error.textContent : '',
            feedGone: !feed,
          };
        })()`).catch(() => null);
        if (
          errorView?.path.endsWith(`/coding-agent/runs/${MISSING_RUN_ID}`) &&
          errorView.errorPresent &&
          errorView.errorRect &&
          errorView.errorRect.width > 0 &&
          errorView.errorRect.height > 0 &&
          errorView.feedGone
        ) {
          break;
        }
      }
      if (
        !errorView?.path.endsWith(`/coding-agent/runs/${MISSING_RUN_ID}`) ||
        !errorView.errorPresent ||
        !errorView.errorRect ||
        errorView.errorRect.width <= 0 ||
        errorView.errorRect.height <= 0 ||
        !errorView.feedGone
      ) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `navigating to /coding-agent/runs/${MISSING_RUN_ID} did not surface a visible ` +
            `DispatchRunError and hide the feed: ${JSON.stringify(errorView)} ` +
            `pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }

      // ---- No fatal page errors may survive the run -----------------------
      const pageErrors = await readPageErrors(evalInApp);
      const fatal = pageErrors.filter((e) => FATAL_ERROR_RE.test(e));
      if (fatal.length > 0) {
        fail(`fatal page errors at end of run: ${JSON.stringify(fatal)}`);
        return;
      }

      console.log(
        `ok: DispatchRunScreen mounted at ${transcriptView.path} with a ` +
          `${Math.round(transcriptView.feedHeight)}px DispatchRunFeed carrying the prompt ` +
          `excerpt ('${FINISHED_PROMPT_EXCERPT}'), the first assistant text ` +
          `('${FINISHED_FIRST_ASSISTANT_TEXT}') and the three tool names ` +
          `[${FINISHED_TOOL_NAMES.join(', ')}]; DispatchRunHeader carried both ` +
          `'${FINISHED_TOKENS_TEXT}' (tokens) and '${FINISHED_COST_TEXT}' (cost); no ` +
          `ThreadComposer/ComposerSendButton was mounted; DispatchRunDashboardLink rendered ` +
          `with width=${Math.round(transcriptView.dashboardLinkWidth)}px (> ${DASHBOARD_LINK_MIN_WIDTH_PX}px); ` +
          `the opencode-log fake recorded ${detailCalls.length} GET ` +
          `/opencode-log/api/runs/fx-run-finished call(s); navigating to ` +
          `/coding-agent/runs/${MISSING_RUN_ID} surfaced DispatchRunError ` +
          `('${errorView.errorText.slice(0, 80)}') with a ` +
          `${Math.round(errorView.errorRect.width)}x${Math.round(errorView.errorRect.height)} ` +
          `box and hid the feed; no Minified-React-error/isReady page errors.`,
      );
    }
  );
} finally {
  await logServer.stop();
  await wsServer.stop();
}
