/**
 * Walks the leetcode and coding-agent flows and captures a screenshot at
 * every step, then writes self-contained HTML reports with the images inlined.
 *
 * This exists because the e2e suite asserts and discards: it proves geometry
 * and text, then throws the pixels away. A reviewer wants to SEE the sheet,
 * and "the assertion passed" is not the same evidence as a picture of it.
 *
 * Runs the same flow on both an iPhone and a Pixel, because the keyboard
 * strategy differs between them and the difference is visual. The coding-agent
 * walk runs for both light and dark color schemes so the report can show the
 * same moment in each scheme side by side.
 *
 * The coding-agent walk boots against a paused fake WS server
 * (frontend/mla-tamagui/features/coding-agent/src/testing/node/fakeWsAgentServer.mjs) so the empty list
 * and the reconnecting banner are genuine states, then resumes it to fill the
 * list with the fixture threads. Every thread-route state the contract names
 * (composer, streaming feed, tool-group collapse/expand, diff, approval,
 * question, plan composer/card, model picker, error) is reached by tapping the
 * fixture row and polling for ThreadScreen -- the same `navigateIntoThread`
 * helper the honest verify-coding-agent-*.mjs checks use -- so the captures
 * reflect the actual rendered surfaces, not the closest reachable list twin.
 * A state whose selector never appears throws with the state name rather than
 * silently skipping the capture.
 */
import { spawn } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { APP_DIR, fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';
import { startFakeOpencodeLogServer } from '../features/coding-agent/src/testing/node/fakeOpencodeLogServer.mjs';

const OUT_DIR = join(APP_DIR, '.report');

/** @type {{device: string, scheme: string, title: string, note: string, file: string}[]} */
const shots = [];

async function capture(ctx, device, scheme, title, note) {
  const result = await ctx.container.screenshot({ name: ctx.name });
  if (!result?.path) {
    fail(`screenshot failed for "${title}"`);
    return;
  }
  const file = `${device}-${scheme}-${String(shots.length).padStart(2, '0')}.png`;
  writeFileSync(join(OUT_DIR, file), readFileSync(result.path));
  shots.push({ device, scheme, title, note, file });
  console.log(`  · ${title}`);
}

const settle = (ms = 900) => new Promise((r) => setTimeout(r, ms));

/** Polls until `expression` is truthy, so a capture never races a transition. */
async function waitFor(ctx, expression, attempts = 16) {
  for (let i = 0; i < attempts; i++) {
    if (await ctx.evalInApp(expression)) return true;
    await settle(400);
  }
  return false;
}

async function walk(ctx, device, scheme) {
  const isIos = device.startsWith('iphone');

  await settle(1200);
  await capture(ctx, device, scheme, 'Problem list — cold start',
    'The Blind 75 hero over the grouped inset list. 72 problems in 11 categories, straight from the catalog.');

  await ctx.evalInApp(`(() => {
    const sv = appDoc.querySelector('[data-testid="LeetcodeListScroll"]');
    if (sv) sv.scrollTop = 420;
    return 1;
  })()`);
  await settle();
  await capture(ctx, device, scheme, 'Problem list — scrolled',
    'The hero has scrolled away and the compact bar has faded in over it. Rows stay tappable throughout, which an earlier overlay version of this header broke.');

  // The compact bar's button, not the hero's: at this scroll offset the hero
  // is off-screen, and a comma-selector would pick it (first in DOM order) and
  // fail the hit test with "nothing at the element centre".
  await ctx.tapInApp('[data-testid="LeetcodeListMenuButtonCompact"]');
  await settle(1100);
  await capture(ctx, device, scheme, 'Drawer',
    'Two destinations and a title. App #1 earns more here; this one does not.');

  await ctx.tapInApp('[data-testid="DrawerItemShowcase"]');
  await settle(1400);
  await capture(ctx, device, scheme, 'Design system showcase',
    'The Tamagui primitives the feature is built from — button variants, badges, the grouped inset list, a text field.');

  // Re-navigate rather than tapping the drawer's Home item: the drawer closed
  // itself on navigation, so that item is no longer hit-testable, and a
  // swallowed failure there left the walk stranded on the showcase.
  await ctx.goto('');
  if (!(await waitFor(ctx, `!!appDoc.querySelector('[data-testid="LeetcodeListItem"]')`))) {
    fail('could not get back to the problem list after the showcase');
    return;
  }

  await ctx.container.tap({ name: ctx.name, ref: '[data-testid="LeetcodeListItem"]' });
  await waitFor(ctx, `!!appDoc.querySelector('[data-testid="ProblemEditorScreen"]')`);
  await settle(1200);
  await capture(ctx, device, scheme, 'Editor — opened',
    'CodeMirror in a WebView with the problem title in the header, language picker, action row and the custom test input.');

  // Before the keyboard goes up: the header is unobstructed here, and the
  // popover has the whole screen to anchor into.
  await ctx.tapInApp('[data-testid="ProblemEditorMenuButton"]');
  await waitFor(
    ctx,
    `!!appDoc.querySelector('[data-testid="ProblemEditorMenuButtonContent"]')`
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Header menu',
    'The overflow menu behind the three dots, anchored under its own trigger. It portals out of the navigator header, which renders outside the SafeAreaProvider subtree — the arrangement that took the whole screen down when this header first used the throwing safe-area hook.');

  // Toggle the trigger to dismiss. NOT an Escape dispatch at the window: that
  // closed the popover but cleared the editor's focus with it, so the keyboard
  // never came back up and the rails never mounted two steps later.
  await ctx.tapInApp('[data-testid="ProblemEditorMenuButton"]');
  await settle(700);

  await ctx.container.tap({ name: ctx.name, ref: '.cm-content' });
  await waitFor(ctx, `!!appDoc.querySelector('[data-testid="EditorHelperRails"]')`);
  await settle(1100);
  await capture(ctx, device, scheme, `Editor — keyboard up (${isIos ? 'iOS overlay' : 'Android resize'})`,
    isIos
      ? 'iOS keeps the layout viewport at full height and draws the keyboard over it, so the app must inset itself. Both toolbar rails sit clear of the keyboard because it does.'
      : 'Android shrinks the layout viewport, so the rails ride up flush against the keyboard without the app doing anything.');

  await ctx.container.type({ name: ctx.name, ref: '.cm-content', text: 'nums_seen = {}' });
  await settle(900);
  await capture(ctx, device, scheme, 'Editor — typed into CodeMirror',
    'Characters typed through the mock keyboard land in CodeMirror with syntax highlighting, proving the postMessage bridge is live rather than just loaded.');

  // A real key id from the rail, and NOT swallowed. The first version tapped
  // "EditorSyntax-Indent", which does not exist -- the id is "Tab" -- and the
  // `.catch()` hid it, so the report shipped two byte-identical screenshots
  // labelled as different steps.
  const before = await ctx.evalInApp(`(() => {
    const f = appDoc.querySelector('[data-testid="ProblemEditorEditorZone"] iframe');
    const cm = f && f.contentDocument && f.contentDocument.querySelector('.cm-content');
    return cm ? cm.textContent.length : -1;
  })()`);
  await ctx.tapInApp('[data-testid="EditorSyntax-Paren"]');
  await settle(900);
  const after = await ctx.evalInApp(`(() => {
    const f = appDoc.querySelector('[data-testid="ProblemEditorEditorZone"] iframe');
    const cm = f && f.contentDocument && f.contentDocument.querySelector('.cm-content');
    return cm ? cm.textContent.length : -1;
  })()`);
  if (!(after > before)) {
    fail(`the toolbar key inserted nothing (document stayed at ${before} chars)`);
    return;
  }
  await capture(ctx, device, scheme, 'Editor — toolbar key pressed',
    `A rail key inserts at the caret and the keyboard stays up (${before} → ${after} chars).`);

  await ctx.tapInApp('[data-testid="ProblemEditorInfoButton"]');
  await waitFor(ctx, `!!appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]')`);
  await settle(1200);
  await capture(ctx, device, scheme, 'Bottom sheet — problem description',
    'A Tamagui sheet over a dimmed backdrop with a rounded top edge and a handle. Without the explicit dim it was white-on-white and read as the page rather than a surface above it.');

  await ctx.evalInApp(`(() => {
    const sv = appDoc.querySelector('[data-testid="ProblemDescriptionScroll"]');
    if (sv) sv.scrollTop = 300;
    return 1;
  })()`);
  await settle();
  await capture(ctx, device, scheme, 'Bottom sheet — scrolled',
    'Examples render as fenced code blocks; the sheet body scrolls independently of the page behind it.');

  // Version history, with something actually IN it. A language switch is the
  // cheapest way to create a version -- it discards the buffer outright, which
  // is precisely the data loss the store exists to undo -- and unlike Format it
  // needs no backend, so this works against a static export.
  await ctx.evalInApp(
    `(() => { appDoc.defaultView.history.back(); return 1; })()`
  );
  await waitFor(ctx, `!!appDoc.querySelector('[data-testid="ProblemEditorScreen"]')`);
  await settle(900);
  // The picker is a wheel in a sheet now, so the language row needs the sheet
  // opened before it exists. Its per-language testID is unchanged on purpose.
  await ctx.tapInApp('[data-testid="ProblemEditorLanguagePicker-Trigger"]');
  await waitFor(
    ctx,
    `!!appDoc.querySelector('[data-testid="ProblemEditorLanguagePicker-JavaScript"]')`
  );
  await settle(500);
  await ctx.tapInApp('[data-testid="ProblemEditorLanguagePicker-JavaScript"]');
  await settle(1100);

  await ctx.tapInApp('[data-testid="ProblemEditorMenuButton"]');
  await waitFor(
    ctx,
    `!!appDoc.querySelector('[data-testid="ProblemEditorMenuButtonContent"]')`
  );
  await settle(600);
  // `tapInApp` hit-tests and dispatches in ONE eval, so a tap that navigates --
  // as this one does -- races the teardown of the frame it ran in. A detach
  // here means the tap landed; anything else is a real failure.
  try {
    await ctx.tapInApp('[data-testid="ProblemEditorMenuHistory"]');
  } catch (err) {
    if (!/detached/i.test(String(err))) throw err;
  }
  if (!(await waitFor(ctx, `!!appDoc.querySelector('[data-testid="CodeHistoryRow"]')`))) {
    fail('the language switch did not leave a version to show in the history sheet');
    return;
  }
  await settle(900);
  await capture(ctx, device, scheme, 'Version history',
    'A version saved automatically before the language switch discarded the buffer. Reset used to be genuinely irreversible here — its dialog still says "This cannot be undone" because, until this store existed, it was true.');

  // Same reason as above -- and the debug FAB is global, so any route will do.
  await ctx.goto('');
  await waitFor(ctx, `!!appDoc.querySelector('[data-testid="DebugFab"]')`);

  // DebugFabWrap is the GestureDetector's Animated.View (the tap target);
  // the inner DebugFab button is wrapped in a pointerEvents="none" YStack,
  // so elementFromPoint at the button's centre returns DebugFabWrap instead.
  // Tapping the wrap directly drives the same gesture the user does.
  await ctx.tapInApp('[data-testid="DebugFabWrap"]');
  await waitFor(ctx, `(() => {
    const el = appDoc.querySelector('[data-testid="DebugLogSheet"]');
    return !!el && el.getBoundingClientRect().top < appDoc.defaultView.innerHeight - 40;
  })()`);
  await settle(1200);
  await capture(ctx, device, scheme, 'Bottom sheet — debug log',
    'The live tail: dense hairline-separated rows, newest first. App #1 renders each line as a bordered card with four different font sizes.');
}

/**
 * Captures the coding-agent states for the report, in one color scheme.
 *
 * The fake WS server is started (paused) around the whole capture, so the app
 * boots into the empty state and the list fills only when the server resumes
 * -- both the "list empty" and "reconnecting banner" moments are genuine.
 *
 * Every thread-route surface the contract names is reached by tapping the
 * fixture row and polling for `ThreadScreen` -- the same `navigateIntoThread`
 * helper the honest verify-coding-agent-*.mjs checks use -- so the captures
 * reflect the actual rendered screens, not the closest reachable list twin.
 * The capture order is fixed (so `build-report` aligns frames across device
 * and scheme runs); a state whose expected selector never appears throws with
 * the state name rather than silently skipping the capture.
 */

// Thread route selectors the impl actually exposes -- grep frontend/mla-tamagui/features/coding-agent/src/ui.
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const BACK = '[data-testid="ThreadScreenHeaderBack"]';
const COMPOSER_INPUT = '[data-testid="ThreadComposerInput"]';
const COMPOSER_SEND = '[data-testid="ThreadComposerSendButton"]';
const COMPOSER_STOP = '[data-testid="ThreadComposerStopButton"]';
const SETTINGS_BUTTON = '[data-testid="ThreadSettingsButton"]';
const SETTINGS_SHEET = '[data-testid="ThreadSettingsSheet"]';
const PLAN_SEGMENT = '[data-testid="InteractionModeSegment-plan"]';
const PLAN_PLACEHOLDER = 'Describe what to plan…';

const THREAD_ROW = (id) => `[data-testid="ThreadRow-${id}"]`;

/**
 * Taps a thread-row testID and polls for the thread route. Returns the
 * resolved state on success; throws (with the row id) if the URL never lands
 * or the ThreadScreen box never grows. Scrolls the row into view first --
 * the FlatList keeps the previous scroll position when returning to the list,
 * so a row that was previously off-screen stays off-screen until something
 * scrolls it back, and tapInApp refuses "nothing at the centre".
 */
async function navigateIntoThread(ctx, rowId) {
  const selector = THREAD_ROW(rowId);
  await ctx.evalInApp(`(() => {
    const el = appDoc.querySelector(${JSON.stringify(selector)});
    if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center', inline: 'nearest' });
    return !!el;
  })()`).catch(() => null);
  await settle(300);
  await ctx.tapInApp(selector);
  for (let attempt = 0; attempt < 12; attempt++) {
    const state = await ctx.evalInApp(`(() => {
      const screen = appDoc.querySelector(${JSON.stringify(THREAD_SCREEN)});
      const rect = screen ? screen.getBoundingClientRect() : null;
      return {
        path: appDoc.location.pathname,
        screenBox: !!(rect && rect.width > 0 && rect.height > 0),
      };
    })()`).catch(() => null);
    if (state?.path.endsWith(`/coding-agent/${rowId}`) && state.screenBox) return state;
    await settle(400);
  }
  throw new Error(
    `tapping ${selector} did not reach /coding-agent/${rowId} with a non-zero ThreadScreen box`,
  );
}

/**
 * Taps the header back control and polls for the list with its rows. Throws
 * if the list never reappears -- a silent failure here would leave the walk
 * stranded on a half-mounted thread route.
 */
async function backToList(ctx) {
  try {
    await ctx.tapInApp(BACK);
  } catch (err) {
    if (!/detached/i.test(String(err))) throw err;
  }
  for (let attempt = 0; attempt < 20; attempt++) {
    const state = await ctx.evalInApp(`(() => ({
      path: appDoc.location.pathname,
      rowCount: appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length,
    }))()`).catch(() => null);
    if (state && /\/coding-agent$/.test(state.path) && state.rowCount > 0) return state;
    await settle(400);
  }
  throw new Error('the header back control did not return to /coding-agent with its rows');
}

/**
 * Polls for `expression`; throws with the state name on miss so a missing
 * selector aborts the walk loudly rather than silently skipping the capture.
 */
async function waitForState(ctx, stateName, expression, attempts = 16) {
  if (await waitFor(ctx, expression, attempts)) return;
  throw new Error(`state "${stateName}" never appeared (selector: ${expression})`);
}

async function walkCodingAgent(ctx, device, scheme, server) {
  // ---- route to coding-agent -----------------------------------------
  // The light-scheme run follows the leetcode walk, which lands on the
  // problem list; the dark-scheme run boots on the same page. Either way
  // we need the coding-agent route before its list testIDs exist.
  await ctx.goto('coding-agent');
  await waitForState(
    ctx,
    'Route — coding-agent',
    `!!appDoc.querySelector('[data-testid="ThreadListScreen"], [data-testid="ThreadList"]')`,
  );

  // ---- list empty -----------------------------------------------------
  // The server is paused at boot, so the shell snapshot never lands and the
  // list stays on ThreadListEmpty behind the connecting banner.
  await waitForState(
    ctx,
    'List — empty',
    `!!appDoc.querySelector('[data-testid="ThreadListEmpty"]')`,
  );
  await settle(900);
  await capture(ctx, device, scheme, 'List — empty',
    'Booted against a paused WS server, so the shell snapshot never applies: the empty state with the connecting banner. The list fills the moment the server resumes.');

  // ---- list filled ----------------------------------------------------
  await server.resume();
  await waitForState(
    ctx,
    'List — filled',
    `appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length > 0`,
  );
  await settle(1200);
  await capture(ctx, device, scheme, 'List — filled',
    'All ten fixture threads from the fake server\u2019s snapshot, each with its status dot: streaming, tool-calls, diff, approval, user-input, plan, error and the rest.');

  // ---- dispatch runs section (FlatList footer; scroll into view) -------
  await ctx.evalInApp(`(() => {
    const el = appDoc.querySelector('[data-testid="DispatchRunsSection"]');
    if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center', inline: 'nearest' });
    return !!el;
  })()`).catch(() => null);
  await waitForState(
    ctx,
    'Dispatch runs section',
    `!!appDoc.querySelector('[data-testid="DispatchRunsSection"]')`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Dispatch runs section',
    'The FlatList footer at the bottom of the thread list: the opencode-log dispatch runs, each with its agent and model badges, prompt excerpt, status dot, relative time and (when finished) cost.');

  // ---- composer focused (fx-basic: completed turn, composer renders Send) --
  await navigateIntoThread(ctx, 'fx-thread-fx-basic');
  await waitForState(
    ctx,
    'Composer — focused',
    `!!appDoc.querySelector(${JSON.stringify(COMPOSER_INPUT)})`,
  );
  // tapInApp's hit-tested click on the TextInput focuses it on web; an explicit
  // .focus() belt-and-braces for any focus-trap ancestor that ate the event.
  await ctx.evalInApp(`(() => {
    const el = appDoc.querySelector(${JSON.stringify(COMPOSER_INPUT)});
    if (el && el.focus) el.focus();
    return !!(appDoc.activeElement === el || (el && el.contains && el.contains(appDoc.activeElement)));
  })()`);
  await settle(700);
  await capture(ctx, device, scheme, 'Composer — focused',
    'The composer with focus on the input: the Send button is live (fx-basic is a completed turn), placeholder and chip rail visible. The keyboard would normally be here; the device-frame emulator only shows it after a typed keystroke, so the focus ring is the proof.');

  // ---- streaming (fx-streaming: latestTurn.state=running) -------------
  await backToList(ctx);
  await navigateIntoThread(ctx, 'fx-thread-fx-streaming');
  await waitForState(
    ctx,
    'Streaming',
    `!!appDoc.querySelector(${JSON.stringify(COMPOSER_STOP)})`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Streaming',
    'The streaming fixture thread: latestTurn.state=running so the composer renders Stop instead of Send, and the live assistant message renders the streaming caret above the fold.');

  // ---- tool group collapsed (fx-tool-calls: 8 entries, 1 visible) ----
  await backToList(ctx);
  await navigateIntoThread(ctx, 'fx-thread-fx-tool-calls');
  await waitForState(
    ctx,
    'Tool group — collapsed',
    `appDoc.querySelectorAll('[data-testid^="ToolRow-"]').length >= 1`,
  );
  await waitForState(
    ctx,
    'Tool group — collapsed',
    `!!appDoc.querySelector('[data-testid^="WorkGroupToggle-"]')`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Tool group — collapsed',
    'The heavy tool run fixture with its eight work-log entries folded into one visible row + a "+7 more" WorkGroupToggle. This is MAX_VISIBLE_WORK_LOG_ENTRIES doing its job.');

  // ---- tool group expanded -------------------------------------------
  // The WorkGroupToggle label flips from "+N previous tool calls" to "Hide N
  // tool calls" when the store's `expandedGroupIds` set gains the group --
  // the visible row count stays the same (buildThreadFeed does not currently
  // re-derive against the store, a known impl gap), so the proof the tap
  // landed is the label, not the row count.
  await ctx.tapInApp('[data-testid^="WorkGroupToggle-"]');
  await waitForState(
    ctx,
    'Tool group — expanded',
    `(() => {
      const el = appDoc.querySelector('[data-testid^="WorkGroupToggle-"]');
      return !!(el && /Hide\\s+\\d+\\s+tool\\s+calls/.test(el.textContent || ''));
    })()`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Tool group — expanded',
    'Same fixture after tapping the WorkGroupToggle: the chip label flips to "Hide 7 tool calls", proving the toggle round-tripped through the store. The feed rows themselves stay collapsed behind MAX_VISIBLE_WORK_LOG_ENTRIES until the impl re-derives the feed against expandedGroupIds.');

  // ---- diff (fx-diff: 1 tool row whose expanded form renders DiffView) --
  await backToList(ctx);
  await navigateIntoThread(ctx, 'fx-thread-fx-diff');
  await waitForState(
    ctx,
    'Diff',
    `appDoc.querySelectorAll('[data-testid^="ToolRow-"]').length >= 1`,
  );
  // Expand the diff tool row -- its handler mounts ToolRowDiff-* with the
  // parsed unified-diff rows (additions in green-tinted, deletions in red).
  await ctx.tapInApp('[data-testid^="ToolRow-"]');
  await waitForState(
    ctx,
    'Diff',
    `appDoc.querySelectorAll('[data-testid^="ToolRowDiff-"]').length >= 1`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Diff',
    'The diff fixture with the work-log row expanded: the design-system DiffView renders the unified-diff add/del rows (export const b = 2 -> 3 in foo.ts, "y" -> "z" in bar.ts) right in the feed, distinct backgrounds on the two sides.');

  // ---- approval pending (fx-approval: 2 pending request.opened cards) -
  await backToList(ctx);
  await navigateIntoThread(ctx, 'fx-thread-fx-approval');
  await waitForState(
    ctx,
    'Approval — pending',
    `appDoc.querySelectorAll('[data-testid^="ApprovalCard-"]').length >= 1`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Approval — pending',
    'The approval fixture thread with its two pending command-approval cards stacked above the composer: each card names the command (pnpm deploy:prod / pnpm publish), the Allow once / Always / Deny row sits underneath.');

  // ---- approval approved (Allow once -> thread.approval.respond wire) -
  // The fixture impl does not optimistically remove the card on tap -- it
  // dispatches thread.approval.respond(decision=accept) and only the server's
  // reply (or a future snapshot) drops the pending entry. The visible proof
  // the tap landed is the wire, not the card count: same contract the honest
  // verify-coding-agent-approval.mjs check enforces.
  await ctx.evalInApp(`(() => {
    const el = appDoc.querySelector('[data-testid^="ApprovalCardAllow-"]');
    if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center', inline: 'nearest' });
    return !!el;
  })()`).catch(() => null);
  await settle(300);
  await ctx.tapInApp('[data-testid^="ApprovalCardAllow-"]').catch((err) => {
    throw new Error(`Approval — approved: tapping ApprovalCardAllow threw: ${err?.message ?? err}`);
  });
  let approved = false;
  let lastReceived = [];
  for (let attempt = 0; attempt < 16; attempt++) {
    try {
      const res = await fetch(`${server.httpUrl}/__received`);
      if (res.ok) {
        lastReceived = await res.json();
        if (
          Array.isArray(lastReceived) &&
          lastReceived.some(
            (c) => c && c.type === 'thread.approval.respond' && c.decision === 'accept',
          )
        ) {
          approved = true;
          break;
        }
      }
    } catch {
      // server might be restarting its connection; retry
    }
    await settle(400);
  }
  if (!approved) {
    const pageErrors = await ctx.evalInApp(
      `JSON.stringify((appDoc.defaultView && appDoc.defaultView.__pageErrors) || [])`,
    ).catch(() => '[]');
    const parsed = typeof pageErrors === 'string' ? JSON.parse(pageErrors) : pageErrors;
    throw new Error(
      `state "Approval — approved" never appeared: tapping ApprovalCardAllow did not put ` +
        `a thread.approval.respond(decision=accept) on the wire within the timeout. ` +
        `lastReceived=${JSON.stringify(lastReceived)} pageErrors=${JSON.stringify(parsed)}`,
    );
  }
  await settle(700);
  await capture(ctx, device, scheme, 'Approval — approved',
    'Same fixture after tapping the first card\'s Allow once: the wire now holds a thread.approval.respond(decision=accept) for that requestId; the card itself only unmounts on the next shell snapshot, so the second pending card stays visible here for context.');

  // ---- question (fx-user-input: pending user-input card with options) --
  await backToList(ctx);
  await navigateIntoThread(ctx, 'fx-thread-fx-user-input');
  await waitForState(
    ctx,
    'Question',
    `appDoc.querySelectorAll('[data-testid^="UserInputCard-"]').length >= 1`,
  );
  await waitForState(
    ctx,
    'Question',
    `appDoc.querySelectorAll('[data-testid^="UserInputCardOption-"]').length >= 2`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Question',
    'The user-input fixture thread with the two-question card: q-auth offers OAuth / API key, q-scopes offers read / write / admin. Submit is disabled until every question has an answer.');

  // ---- plan composer (fx-basic + InteractionModeSegment-plan) ---------
  await backToList(ctx);
  await navigateIntoThread(ctx, 'fx-thread-fx-basic');
  await waitForState(
    ctx,
    'Plan — composer',
    `!!appDoc.querySelector(${JSON.stringify(COMPOSER_SEND)})`,
  );
  await ctx.tapInApp(PLAN_SEGMENT);
  await waitForState(
    ctx,
    'Plan — composer',
    `(() => { const el = appDoc.querySelector(${JSON.stringify(COMPOSER_INPUT)}); return (el && el.placeholder) === ${JSON.stringify(PLAN_PLACEHOLDER)}; })()`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Plan — composer',
    'The composer after tapping the Plan segment: the placeholder flips from "Message…" to "Describe what to plan…", the InteractionModeSegment reflects the selection, and the composer is wired to surface a PlanCard on the next turn.');

  // ---- plan card (send fixture:plan, wait for PlanCard) ---------------
  // Re-focus the composer first -- tapping the Plan segment moves focus away
  // and container.type can silently miss a re-rendering input.
  await ctx.evalInApp(`(() => {
    const el = appDoc.querySelector(${JSON.stringify(COMPOSER_INPUT)});
    if (el && el.focus) el.focus();
    return 1;
  })()`);
  await settle(300);
  await ctx.container.type({ name: ctx.name, ref: COMPOSER_INPUT, text: 'fixture:plan' });
  await settle(400);
  // DebugFab overlaps the Send button's hit-test; dispatch the same
  // pointer/mouse sequence the clickThroughDebugFab pattern uses.
  const sendOutcome = await ctx.evalInApp(`(() => {
    const el = appDoc.querySelector(${JSON.stringify(COMPOSER_SEND)});
    if (!el) return { ok: false, reason: 'no send button' };
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
  if (!sendOutcome?.ok) {
    throw new Error(`Plan — card: tapping Send failed: ${sendOutcome?.reason ?? 'unknown'}`);
  }
  await waitForState(
    ctx,
    'Plan — card',
    `appDoc.querySelectorAll('[data-testid^="PlanCard-"]').length >= 1`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Plan — card',
    'Same thread after sending "fixture:plan": a PlanCard renders in the feed under a "Turn · completed" fold with the proposed steps and an Implement button.');

  // ---- model picker (fx-basic + ThreadSettingsButton -> sheet) --------
  await backToList(ctx);
  await navigateIntoThread(ctx, 'fx-thread-fx-basic');
  await ctx.tapInApp(SETTINGS_BUTTON);
  await waitForState(
    ctx,
    'Model picker',
    `(() => {
      const el = appDoc.querySelector(${JSON.stringify(SETTINGS_SHEET)});
      if (!el) return false;
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    })()`,
  );
  await settle(700);
  await capture(ctx, device, scheme, 'Model picker',
    'The thread-settings sheet over a dimmed backdrop, listing the opencode and claude model groups with their per-model ListItems. codex is hidden because the probe marks it unavailable.');

  // Close the sheet before resuming so the reconnecting-banner step sees the
  // list and not a still-open sheet (the sheet has no explicit close in the
  // App.testIDs we polled for; tapping the backdrop is what the user does).
  await ctx.evalInApp(`(() => {
    const v = appDoc.defaultView;
    const sheet = appDoc.querySelector(${JSON.stringify(SETTINGS_SHEET)});
    if (!sheet) return false;
    const r = sheet.getBoundingClientRect();
    // Tap well above the sheet (top edge minus 60px) so the dismiss lands
    // on the dimmed backdrop instead of one of the controls inside the sheet.
    const x = r.left + r.width / 2;
    const y = Math.max(8, r.top - 60);
    const opts = { bubbles: true, cancelable: true, view: v, button: 0, clientX: x, clientY: y };
    const target = appDoc.elementFromPoint(x, y) || appDoc.body;
    target.dispatchEvent(new v.PointerEvent('pointerdown', { ...opts, pointerId: 1, isPrimary: true }));
    target.dispatchEvent(new v.MouseEvent('mousedown', opts));
    target.dispatchEvent(new v.PointerEvent('pointerup', { ...opts, pointerId: 1, isPrimary: true }));
    target.dispatchEvent(new v.MouseEvent('mouseup', opts));
    target.dispatchEvent(new v.MouseEvent('click', opts));
    return true;
  })()`);
  await waitForState(
    ctx,
    'Model picker',
    `!appDoc.querySelector(${JSON.stringify(SETTINGS_SHEET)})`,
  ).catch(() => {
    // The sheet may render without its data-testID while it's dismissing; the
    // backdrop dismiss path is best-effort, so accept whatever the next step
    // sees rather than failing the walk on a cosmetic overlay.
  });

  // ---- reconnecting banner -------------------------------------------
  // Back on the list, drop the socket so the gateway flips to reconnecting and
  // the banner appears over the filled list. This is the only state that needs
  // the server paused mid-walk.
  await backToList(ctx);
  await server.pause();
  await waitForState(
    ctx,
    'Reconnecting banner',
    `!!appDoc.querySelector('[data-testid="ConnectionBanner"]')`,
  );
  await settle(900);
  await capture(ctx, device, scheme, 'Reconnecting banner',
    'The WS socket closed mid-session (fake server pause()): the list keeps its threads while the warning card explains the agent is reconnecting, with a Retry button.');
  await server.resume();

  // ---- error (fx-error: latestTurn.state=error, red dot in the row) ---
  // The feed renders the runtime-error activity as null (it is the row-level
  // status dot and the page subtitle that carry the visual), so the contract
  // we assert on here is the route itself landing on the error thread and
  // ThreadScreen coming up with a non-zero box.
  await waitForState(
    ctx,
    'Error',
    `appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length > 0`,
  );
  await navigateIntoThread(ctx, 'fx-thread-fx-error');
  await settle(700);
  await capture(ctx, device, scheme, 'Error',
    'The failed-turn fixture thread: latestTurn.state=error so the composer still renders Send (no Stop), the thread row carried a red status dot on the list, and the page subtitle names the failing thread id.');

  // ---- dispatch run transcript (LAST capture; newest PNG in .report) ----
  // Back to the list, scroll the finished dispatch run row into view, tap it
  // to navigate client-side into /coding-agent/runs/fx-run-finished -- the
  // static export does not serve /[runId] directly, so the in-app router is
  // the only path -- then poll for DispatchRunFeed so the capture never races
  // the HTTP fetch.
  await backToList(ctx);
  await ctx.evalInApp(`(() => {
    const el = appDoc.querySelector('[data-testid="DispatchRunRow-fx-run-finished"]');
    if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center', inline: 'nearest' });
    return !!el;
  })()`).catch(() => null);
  await settle(500);
  await ctx.tapInApp('[data-testid="DispatchRunRow-fx-run-finished"]');
  await waitForState(
    ctx,
    'Dispatch run transcript',
    `(() => {
      const screen = appDoc.querySelector('[data-testid="DispatchRunScreen"]');
      const feed = appDoc.querySelector('[data-testid="DispatchRunFeed"]');
      const sRect = screen ? screen.getBoundingClientRect() : null;
      const fRect = feed ? feed.getBoundingClientRect() : null;
      return !!(screen && sRect && sRect.width > 0 && sRect.height > 0 &&
                feed && fRect && fRect.width > 0 && fRect.height > 0);
    })()`,
  );
  await settle(900);
  await capture(ctx, device, scheme, 'Dispatch run transcript',
    'The dispatch-run detail screen for fx-run-finished: the read-only ThreadFeed over buildDispatchRunFeed (prompt excerpt, assistant text, three tool rows), the header carrying model/tokens/cost/duration, and the "Open in dashboard" button. No composer -- the run detail is read-only.');
}

mkdirSync(OUT_DIR, { recursive: true });

const device = process.argv[2] ?? 'iphone-15-pro';
const onlyScheme = process.argv[3];

/** Starts the fake WS server paused and the opencode-log HTTP fake, runs `run`, and always stops both. */
async function withFakeServer(run) {
  const server = await startFakeWsAgentServer({ port: 8787 });
  server.pause();
  const logServer = await startFakeOpencodeLogServer({ port: 8091 });
  try {
    await run(server);
  } finally {
    await logServer.stop();
    await server.stop();
  }
}

function writeManifest(scheme) {
  writeFileSync(
    join(OUT_DIR, `${device}-${scheme}.json`),
    JSON.stringify({ device, scheme, shots }, null, 2)
  );
  console.log(`ok: ${shots.length} screenshots (${scheme}) → ${OUT_DIR}`);
}

// A spawned child runs the dark scheme, then the in-process run does light.
// `withServedApp` ends with process.exit(), so the two schemes cannot share a
// process; running the dark one as a child first lets the existing
// `report:capture` (no scheme arg) produce both manifests per device.
if (onlyScheme === 'dark') {
  await withFakeServer(async (server) => {
    await withServedApp(
      {
        port: 8216,
        name: `report-${device}-dark`,
        devicePreset: device,
        colorScheme: 'dark',
        path: 'coding-agent',
      },
      async (ctx) => {
        console.log(`› capturing ${device} (dark)`);
        shots.length = 0;
        await walkCodingAgent(ctx, device, 'dark', server);
        writeManifest('dark');
      },
    );
  });
} else if (onlyScheme === 'light') {
  await withFakeServer(async (server) => {
    await withServedApp(
      {
        port: 8215,
        name: `report-${device}`,
        devicePreset: device,
        colorScheme: 'light',
      },
      async (ctx) => {
        console.log(`› capturing ${device} (light)`);
        await walk(ctx, device, 'light');
        await walkCodingAgent(ctx, device, 'light', server);
        writeManifest('light');
      },
    );
  });
} else {
  const code = await new Promise((resolve) => {
    const child = spawn(
      process.execPath,
      [fileURLToPath(import.meta.url), device, 'dark'],
      { stdio: 'inherit' },
    );
    child.on('exit', resolve);
  });
  if (code !== 0) process.exit(code || 1);
  await withFakeServer(async (server) => {
    await withServedApp(
      {
        port: 8215,
        name: `report-${device}`,
        devicePreset: device,
        colorScheme: 'light',
      },
      async (ctx) => {
        console.log(`› capturing ${device} (light)`);
        await walk(ctx, device, 'light');
        await walkCodingAgent(ctx, device, 'light', server);
        writeManifest('light');
      },
    );
  });
}
