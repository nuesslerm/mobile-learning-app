/**
 * Phase-9 acceptance (#252, #254) as amended by the tranche-4 honesty fixes
 * (`docs/archive/README.md`): the coding-agent
 * thread surfaces (AssistantMessage, ToolRow, PendingApprovalCard, DiffView
 * rows) keep enough text-vs-background contrast on the dark theme to read.
 *
 * Distinct from `verify-dark-theme.mjs`, which picks the lightest surface and
 * proves the shim repainted; this walks the actual thread surfaces and checks
 * at the WCAG AA body-text threshold of 4.5:1.
 *
 * The dark scheme is forced at the Playwright context
 * (`withServedApp({ colorScheme: 'dark' })`); `prefers-color-scheme` reaches
 * the served app at first paint, so Tamagui resolves tokens in the dark theme
 * without any in-page shim.
 *
 * We navigate into the four fixture threads and assert on the named surface
 * for each:
 *   - fx-thread-fx-basic    -> AssistantMessage
 *   - fx-thread-fx-tools    -> ToolRow
 *   - fx-thread-fx-approval -> PendingApprovalCard
 *   - fx-thread-fx-diff     -> ToolRowDiff (after expanding the tool row)
 * then, as a second step, the showcase's real DS DiffView rows.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';

const PORT = 8212;
const NAME = 'coding-agent-dark-verify';

const LIST = '[data-testid="ThreadList"]';
const THREAD_SCREEN = '[data-testid="ThreadScreen"]';
const BACK = '[data-testid="ThreadScreenHeaderBack"]';
const DIFF_TOOL_ROWS = '[data-testid^="ToolRow-"]';
const FATAL_ERROR_RE = /Minified React error|isReady/;

/** Perceived brightness; the same <128 "this is actually dark" bar verify-dark-theme.mjs uses. */
const LUMA = `(rgb) => {
  const m = /rgba?\\(([^)]+)\\)/.exec(rgb || '');
  if (!m) return null;
  const [r, g, b] = m[1].split(',').map((n) => Number(n.trim()));
  return Math.round(0.2126 * r + 0.7152 * g + 0.0722 * b);
}`;

const problems = [];
function check(name, ok, detail) {
  problems.push({ name, ok, detail });
  console.log(`${ok ? '  ok  ' : '  FAIL'} ${name}${ok ? '' : ` — ${detail}`}`);
}

/** WCAG 2.1 contrast ratio, gamma-corrected. */
const CONTRAST = `(a, b) => {
  const channels = (rgb) => {
    const m = /rgba?\\(([^)]+)\\)/.exec(rgb || '');
    return m ? m[1].split(',').slice(0, 3).map((n) => Number(n.trim())) : null;
  };
  const lum = (rgb) => {
    const c = channels(rgb);
    if (!c) return null;
    const [r, g, bl] = c.map((v) => {
      const s = v / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const la = lum(a), lb = lum(b);
  if (la === null || lb === null) return null;
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}`;

/**
 * Walk up from the text element to the nearest non-transparent
 * background. A depth cap keeps us inside the row.
 *
 * The climb stops at the thread's own page (ThreadScreen): everything above
 * it is the navigator's chrome canvas, not a background the thread paints, so
 * a transparent surface resolves to the app's page background instead.
 */
const SAMPLE_BY_CONTENT = `(needle) => {
  const view = appDoc.defaultView;
  const walker = appDoc.createTreeWalker(appDoc.body, view.NodeFilter.SHOW_TEXT, null);
  let node, textEl = null;
  while ((node = walker.nextNode())) {
    if ((node.nodeValue || '').includes(needle)) { textEl = node.parentElement; break; }
  }
  if (!textEl) return null;
  const textColor = view.getComputedStyle(textEl).color;
  const threadPage = appDoc.querySelector('[data-testid="ThreadScreen"]');
  let bgEl = textEl, bg = null;
  for (let i = 0; i < 24 && bgEl && bgEl !== appDoc.body; i++) {
    if (threadPage && bgEl === threadPage) break;
    const c = view.getComputedStyle(bgEl).backgroundColor;
    if (c && !/rgba\\(0, 0, 0, 0\\)|transparent/.test(c)) { bg = c; break; }
    bgEl = bgEl.parentElement;
  }
  if (!bg && appDoc.body) {
    const bodyBg = view.getComputedStyle(appDoc.body).backgroundColor;
    if (bodyBg && !/rgba\\(0, 0, 0, 0\\)|transparent/.test(bodyBg)) bg = bodyBg;
  }
  return { textColor, bg, text: textEl.textContent.slice(0, 60) };
}`;

// DiffView rows rendered by the real DS component on the showcase.
const SHOWCASE_DIFF = '[data-testid="Showcase-CodingAgent-DiffView"]';
const DIFF_NEEDLES = [
  ['add', 'const name = "world"'],
  ['del', 'return "hi"'],
];

const server = await startFakeWsAgentServer({ port: 8787 });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function readPageErrors(evalInApp) {
  const raw = await evalInApp(
    `JSON.stringify((appDoc.defaultView && appDoc.defaultView.__pageErrors) || [])`,
  ).catch(() => '[]');
  const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
  return Array.isArray(parsed) ? parsed : [];
}

try {
await withServedApp(
  {
    port: PORT,
    name: NAME,
    devicePreset: 'iphone-15-pro',
    colorScheme: 'dark',
    path: 'coding-agent',
  },
  async ({ container, evalInApp, tapInApp, goto }) => {
    const sampleContrast = async (needle) => {
      const sample = await evalInApp(
        `(() => { const s = ${SAMPLE_BY_CONTENT}; return s(${JSON.stringify(needle)}); })()`
      );
      if (!sample?.textColor || !sample?.bg) {
        return { sample, ratio: null };
      }
      const ratio = await evalInApp(
        `(() => { const ratio = ${CONTRAST}; return ratio(${JSON.stringify(sample.textColor)}, ${JSON.stringify(sample.bg)}); })()`
      );
      return { sample, ratio };
    };

    // The dark-scheme precondition: if the body is not actually dark, every
    // contrast number below is measuring the light theme and the gate would
    // pass for the wrong reason.
    const page = await evalInApp(`(() => {
      const luma = ${LUMA};
      const cs = appDoc.defaultView.getComputedStyle(appDoc.body);
      return { background: cs.backgroundColor, luma: luma(cs.backgroundColor) };
    })()`);
    check(
      'the dark color-scheme shim repainted the page',
      page?.luma !== null && page.luma < 128,
      `body background ${page?.background} (luma ${page?.luma}) -- the colour-scheme shim did not take, so nothing below tests the dark theme`
    );

    // ---- list: the four state rows + their title text contrast ---------
    let rowReady = null;
    for (let attempt = 0; attempt < 24; attempt++) {
      await sleep(400);
      rowReady = await evalInApp(`(() => ({
        list: !!appDoc.querySelector(${JSON.stringify(LIST)}),
        rowCount: appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length,
        banner: !!appDoc.querySelector('[data-testid="ConnectionBanner"]'),
      }))()`);
      if (rowReady?.list && rowReady.rowCount > 0) break;
    }
    if (!rowReady?.list || !rowReady.rowCount) {
      fail(`the coding-agent list never rendered in dark mode: ${JSON.stringify(rowReady)}`);
      return;
    }

    // ---- thread surfaces: basic -> AssistantMessage ----------------------
    // navigate in, sample the assistant reply text's contrast, then back out.
    const navigateInto = async (rowSelector, pathSuffix) => {
      await evalInApp(`(() => {
        const el = appDoc.querySelector(${JSON.stringify(rowSelector)});
        if (el) el.scrollIntoView({ block: 'center', inline: 'nearest' });
        return true;
      })()`);
      await sleep(400);
      await tapInApp(rowSelector);
      let ok = null;
      for (let attempt = 0; attempt < 10; attempt++) {
        ok = await evalInApp(`(() => {
          const w = appDoc.defaultView;
          const screen = appDoc.querySelector(${JSON.stringify(THREAD_SCREEN)});
          const r = screen ? screen.getBoundingClientRect() : null;
          return {
            path: appDoc.location.pathname,
            screenBox: !!(r && r.width > 0 && r.height > 0),
          };
        })()`).catch(() => null);
        if (ok?.screenBox && ok.path.endsWith(pathSuffix)) return ok;
        await sleep(400);
      }
      return ok;
    };
    const backToList = async () => {
      await tapInApp(BACK);
      for (let attempt = 0; attempt < 20; attempt++) {
        const back = await evalInApp(`(() => ({
          path: appDoc.location.pathname,
          rowCount: appDoc.querySelectorAll('[data-testid^="ThreadRow-"]').length,
        }))()`).catch(() => null);
        if (back && /\/coding-agent$/.test(back.path) && back.rowCount > 0) return back;
        await sleep(400);
      }
      return null;
    };

    // (1) basic -> AssistantMessage
    const basicThread = await navigateInto(
      '[data-testid="ThreadRow-fx-thread-fx-basic"]',
      '/coding-agent/fx-thread-fx-basic'
    );
    if (!basicThread) {
      fail(`did not reach the basic fixture thread in dark mode`);
      return;
    }
    {
      const { sample, ratio } = await sampleContrast('Hello there, friend');
      check(
        'thread basic: AssistantMessage text vs page background contrast',
        sample !== null && ratio !== null && ratio >= 4.5,
        `text ${sample?.textColor} over ${sample?.bg} (${JSON.stringify(sample?.text)}) -- ratio ${ratio?.toFixed(2)}`
      );
    }

    // (2) tools -> ToolRow
    if (!(await backToList())) {
      fail(`back to the list after the basic thread failed`);
      return;
    }
    const toolsThread = await navigateInto(
      '[data-testid="ThreadRow-fx-thread-fx-tool-calls"]',
      '/coding-agent/fx-thread-fx-tool-calls'
    );
    if (!toolsThread) {
      fail(`did not reach the tools fixture thread in dark mode`);
      return;
    }
    {
      const { sample, ratio } = await sampleContrast('Edit src/foo.test.ts');
      check(
        'thread tools: ToolRow text vs row background contrast',
        sample !== null && ratio !== null && ratio >= 4.5,
        `text ${sample?.textColor} over ${sample?.bg} (${JSON.stringify(sample?.text)}) -- ratio ${ratio?.toFixed(2)}`
      );
    }

    // (3) approval -> PendingApprovalCard
    if (!(await backToList())) {
      fail(`back to the list after the tools thread failed`);
      return;
    }
    const approvalThread = await navigateInto(
      '[data-testid="ThreadRow-fx-thread-fx-approval"]',
      '/coding-agent/fx-thread-fx-approval'
    );
    if (!approvalThread) {
      fail(`did not reach the approval fixture thread in dark mode`);
      return;
    }
    {
      const { sample, ratio } = await sampleContrast('pnpm deploy:prod');
      check(
        'thread approval: PendingApprovalCard text vs card background contrast',
        sample !== null && ratio !== null && ratio >= 4.5,
        `text ${sample?.textColor} over ${sample?.bg} (${JSON.stringify(sample?.text)}) -- ratio ${ratio?.toFixed(2)}`
      );
    }

    // (4) diff -> ToolRowDiff (expand the tool row first)
    if (!(await backToList())) {
      fail(`back to the list after the approval thread failed`);
      return;
    }
    const diffThread = await navigateInto(
      '[data-testid="ThreadRow-fx-thread-fx-diff"]',
      '/coding-agent/fx-thread-fx-diff'
    );
    if (!diffThread) {
      fail(`did not reach the diff fixture thread in dark mode`);
      return;
    }
    let toolRowSeen = 0;
    for (let attempt = 0; attempt < 10; attempt++) {
      toolRowSeen = await evalInApp(
        `appDoc.querySelectorAll('[data-testid^="ToolRow-"]').length`
      ).catch(() => 0);
      if (toolRowSeen >= 1) break;
      await sleep(300);
    }
    if (toolRowSeen < 1) {
      fail(`the diff fixture thread rendered no ToolRow to expand in dark mode`);
      return;
    }
    await evalInApp(`(() => {
      const el = appDoc.querySelector('[data-testid^="ToolRow-"]');
      if (el) el.scrollIntoView({ block: 'center', inline: 'nearest' });
      return true;
    })()`);
    await sleep(300);
    await tapInApp(DIFF_TOOL_ROWS);
    let diffRows = 0;
    for (let attempt = 0; attempt < 8; attempt++) {
      diffRows = await evalInApp(
        `appDoc.querySelectorAll('[data-testid^="ToolRowDiff-"]').length`
      ).catch(() => 0);
      if (diffRows >= 1) break;
      await sleep(300);
    }
    if (diffRows < 1) {
      fail(`expanding the diff tool row revealed no ToolRowDiff in dark mode`);
      return;
    }
    {
      const { sample, ratio } = await sampleContrast('export const b = 3;');
      check(
        'thread diff: ToolRowDiff add row text vs row background contrast',
        sample !== null && ratio !== null && ratio >= 4.5,
        `text ${sample?.textColor} over ${sample?.bg} (${JSON.stringify(sample?.text)}) -- ratio ${ratio?.toFixed(2)}`
      );
    }

    // ---- showcase DiffView rows (second step) ---------------------------
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
      fail(`the showcase DiffView never rendered in dark mode`);
      return;
    }
    for (const [kind, needle] of DIFF_NEEDLES) {
      const { sample, ratio } = await sampleContrast(needle);
      check(
        `showcase DiffView: ${kind} row text vs row background contrast`,
        sample !== null && ratio !== null && ratio >= 4.5,
        `text ${sample?.textColor} over ${sample?.bg} (${JSON.stringify(sample?.text)}) -- ratio ${ratio?.toFixed(2)}`
      );
    }

    // ---- no fatal page errors -------------------------------------------
    const fatal = (await readPageErrors(evalInApp)).filter((e) =>
      FATAL_ERROR_RE.test(e)
    );
    if (fatal.length > 0) {
      fail(`fatal page errors at end of run: ${JSON.stringify(fatal)}`);
      return;
    }

    const failed = problems.filter((p) => !p.ok);
    const shot = await container.screenshot({ name: NAME });
    if (shot?.path) console.log(`screenshot (dark): ${shot.path}`);
    if (failed.length) {
      fail(
        `${failed.length}/${problems.length} dark contrast checks failed: ` +
          failed.map((f) => f.name).join('; ')
      );
      return;
    }
    console.log(
      `ok: ${problems.length} dark contrast checks passed -- the four thread ` +
        `surfaces (AssistantMessage on fx-basic, ToolRow on fx-tools, ` +
        `PendingApprovalCard on fx-approval, ToolRowDiff on fx-diff) plus the ` +
        `showcase's real DiffView add/del rows all clear the 4.5:1 AA threshold; ` +
        `no Minified-React-error/isReady page errors.`
    );
  }
);
} finally {
  await server.stop();
}
