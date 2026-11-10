import path from 'node:path';

import type { Stop } from '../debugger';

declare module 'vitest' {
  // eslint-disable-next-line @typescript-eslint/consistent-type-definitions -- augmentation merges only into an interface
  interface TaskMeta {
    flowSnapshots?: string[];
  }
}

export type FlowReportFrame = {
  file: string;
  line: number;
  column: number;
  method: string;
};
export type FlowReportError = {
  message: string;
  stack?: string;
  frames?: FlowReportFrame[];
  file?: string;
};
export type FlowGolden = {
  step: string;
  snapshot: string;
  golden: string;
  goldenExists: boolean;
  actual: string;
  expected: string;
  diff: string;
};
type FlowTestVerdict = 'passed' | 'failed' | 'skipped' | 'pending';
export type FlowTestReport = {
  file: string;
  name: string;
  verdict: FlowTestVerdict;
  durationMs: number;
  errors: FlowReportError[];
  goldens: FlowGolden[];
  stops: Stop[];
  warnings: string[];
};
export type FlowReport = {
  schema: 'flow-report/1';
  harness: 'flow-test-harness';
  verdict: 'passed' | 'failed' | 'interrupted';
  startedAt: string;
  finishedAt: string;
  durationMs: number;
  root: string;
  paths: { json: string; html: string; debug: string };
  env: { goldens: boolean; theme: string | null };
  summary: {
    total: number;
    passed: number;
    failed: number;
    skipped: number;
    files: number;
    slowest: { file: string; name: string; durationMs: number }[];
  };
  errors: FlowReportError[];
  warnings: string[];
  tests: FlowTestReport[];
};

const ESCAPES: Record<string, string> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
};
const escapeHtml = (value: unknown): string =>
  String(value).replace(/[&<>"']/g, (c) => ESCAPES[c]);

const ms = (n: number): string =>
  n >= 1000 ? `${(n / 1000).toFixed(1)} s` : `${Math.round(n)} ms`;

function hrefFor(report: FlowReport, file: string): string {
  const from = path.dirname(path.resolve(report.root, report.paths.html));
  const rel = path.relative(from, path.resolve(report.root, file));
  return rel.split(path.sep).map(encodeURIComponent).join('/');
}

const link = (report: FlowReport, file: string, label: string): string =>
  `<a href="${escapeHtml(hrefFor(report, file))}">${escapeHtml(label)}</a>`;

function errorBlock(error: FlowReportError): string {
  const where = error.frames?.[0]
    ? `<p class="where">at ${escapeHtml(`${error.frames[0].file}:${error.frames[0].line}:${error.frames[0].column}`)}</p>`
    : '';
  const file = error.file
    ? `<p class="where">${escapeHtml(error.file)}</p>`
    : '';
  return `<div class="error">${file}<pre class="message">${escapeHtml(error.message)}</pre>${where}${error.stack ? `<details><summary>stack</summary><pre>${escapeHtml(error.stack)}</pre></details>` : ''}</div>`;
}

function goldenBlock(report: FlowReport, g: FlowGolden): string {
  // the screenshot this run took, painted by `wf flow run` after vitest; the committed golden is a link beside it
  const img = `<img src="${escapeHtml(hrefFor(report, g.actual))}" alt="${escapeHtml(`screenshot ${g.step}`)}" loading="lazy">`;
  const links = [
    link(report, g.snapshot, 'snapshot html'),
    g.goldenExists ? link(report, g.golden, 'golden png') : '',
    link(report, g.actual, 'actual'),
    // a comparison writes expected and diff; a plain run compared nothing
    report.env.goldens ? link(report, g.expected, 'expected') : '',
    report.env.goldens ? link(report, g.diff, 'diff') : '',
  ].filter(Boolean);
  return `<figure class="golden">${img}<figcaption><code>${escapeHtml(g.step)}</code><span class="links">${links.join(' · ')}</span></figcaption></figure>`;
}

function stopBlock(stop: Stop): string {
  const stack = stop.stack
    .map(
      (f) =>
        `<li><code>${escapeHtml(`${f.file}:${f.line}`)}</code> ${escapeHtml(f.function || '(anonymous)')}</li>`
    )
    .join('');
  return `<div class="stop"><p><span class="tag">${escapeHtml(stop.kind)}</span> <code>${escapeHtml(`${stop.file}:${stop.line}`)}</code> ${escapeHtml(stop.function || '(anonymous)')}</p><pre>${escapeHtml(JSON.stringify(stop.values, null, 2))}</pre><details><summary>stack (${stop.stack.length})</summary><ol class="stack">${stack}</ol></details></div>`;
}

function testBlock(report: FlowReport, t: FlowTestReport): string {
  const body = [
    ...t.errors.map(errorBlock),
    t.goldens.length
      ? `<h4>Goldens</h4><div class="goldens">${t.goldens.map((g) => goldenBlock(report, g)).join('')}</div>`
      : '',
    t.stops.length
      ? `<h4>Debugger stops</h4>${t.stops.map(stopBlock).join('')}`
      : '',
    t.warnings.length
      ? `<h4>Warnings (${t.warnings.length})</h4><pre class="warn">${escapeHtml(t.warnings.join('\n'))}</pre>`
      : '',
  ].join('');
  const open = t.verdict === 'failed' ? ' open' : '';
  return `<details class="test ${t.verdict}"${open}><summary><span class="dot" aria-hidden="true"></span><span class="name">${escapeHtml(t.name)}</span><span class="verdict">${t.verdict}</span><span class="time">${ms(t.durationMs)}</span></summary>${body || '<p class="muted">no errors, goldens or stops</p>'}</details>`;
}

const CSS = `
:root{--bg:#f7f7f8;--panel:#fff;--text:#1d1d1f;--muted:#6b6b73;--line:#e2e2e6;--pass:#1a7f37;--fail:#c62828;--skip:#8a6d00;--code:#f0f0f3;color-scheme:light dark}
@media (prefers-color-scheme:dark){:root{--bg:#121214;--panel:#1c1c20;--text:#ececf0;--muted:#9d9da8;--line:#2e2e35;--pass:#4ac26b;--fail:#ff6b6b;--skip:#e3b341;--code:#26262c}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:15px/1.45 system-ui,-apple-system,sans-serif}
main{max-width:1080px;margin:0 auto;padding:16px}
h1{font-size:1.4rem;margin:0 0 4px}h2{font-size:1rem;margin:24px 0 8px;word-break:break-all}h3{font-size:1rem;margin:20px 0 8px}h4{font-size:.85rem;margin:12px 0 6px;color:var(--muted);text-transform:uppercase;letter-spacing:.04em}
.muted,.where{color:var(--muted);font-size:.85rem;margin:4px 0}
.pill{display:inline-block;padding:2px 10px;border-radius:999px;font-weight:600;color:#fff}.pill.passed{background:var(--pass)}.pill.failed,.pill.interrupted{background:var(--fail)}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(110px,1fr));gap:8px;margin:12px 0}
.stat{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:10px}.stat b{display:block;font-size:1.3rem}.stat span{color:var(--muted);font-size:.8rem}
.stat.failed b{color:var(--fail)}.stat.passed b{color:var(--pass)}
ol.slowest{padding-left:20px;margin:0}ol.slowest li{margin:2px 0;overflow-wrap:anywhere}
.test{background:var(--panel);border:1px solid var(--line);border-left:4px solid var(--line);border-radius:8px;margin:6px 0;padding:0 12px}
.test.passed{border-left-color:var(--pass)}.test.failed{border-left-color:var(--fail)}.test.skipped,.test.pending{border-left-color:var(--skip)}
.test>summary{display:flex;gap:8px;align-items:baseline;padding:10px 0;cursor:pointer;flex-wrap:wrap}
.test>summary .name{flex:1 1 240px;overflow-wrap:anywhere}.verdict,.time{color:var(--muted);font-size:.8rem;white-space:nowrap}
.test.failed .verdict{color:var(--fail);font-weight:600}
.dot{width:8px;height:8px;border-radius:50%;background:var(--line);flex:none;align-self:center}.passed .dot{background:var(--pass)}.failed .dot{background:var(--fail)}.skipped .dot,.pending .dot{background:var(--skip)}
pre{background:var(--code);border-radius:6px;padding:8px;overflow-x:auto;white-space:pre-wrap;overflow-wrap:anywhere;font:12px/1.4 ui-monospace,SFMono-Regular,Menlo,monospace;margin:6px 0}
.error pre.message{border-left:3px solid var(--fail)}pre.warn{max-height:240px;overflow-y:auto}
code{font:12px ui-monospace,SFMono-Regular,Menlo,monospace;overflow-wrap:anywhere}
.goldens{display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:10px}
.golden{margin:0}.golden img{width:100%;border:1px solid var(--line);border-radius:6px;background:#fff}.golden figcaption{font-size:.8rem;display:flex;flex-direction:column;gap:2px}
.stop{border-top:1px dashed var(--line);padding-top:6px}.tag{font-size:.75rem;border:1px solid var(--line);border-radius:4px;padding:0 4px}
ol.stack{margin:4px 0 8px;padding-left:20px;font-size:.85rem}
a{color:inherit}
`;

/** Every screenshot of the run in one grid, failing tests first, so they show without opening each test. */
function screenshotsSection(report: FlowReport): string {
  const shots = [...report.tests]
    .sort(
      (a, b) => Number(b.verdict === 'failed') - Number(a.verdict === 'failed')
    )
    .flatMap((t) => t.goldens);
  if (!shots.length) return '';
  return `<section><h2>Screenshots (${shots.length})</h2><div class="goldens">${shots.map((g) => goldenBlock(report, g)).join('')}</div></section>`;
}

export function renderReportHtml(report: FlowReport): string {
  const s = report.summary;
  const stat = (n: number | string, label: string, cls = '') =>
    `<div class="stat ${cls}"><b>${escapeHtml(n)}</b><span>${escapeHtml(label)}</span></div>`;
  const files = new Map<string, FlowTestReport[]>();
  for (const t of report.tests)
    files.set(t.file, [...(files.get(t.file) ?? []), t]);
  const ordered = [...files].sort(
    ([, a], [, b]) =>
      Number(b.some((t) => t.verdict === 'failed')) -
      Number(a.some((t) => t.verdict === 'failed'))
  );
  const theme = report.env.theme
    ? ` · theme ${escapeHtml(report.env.theme)}`
    : '';
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Flow test report</title><style>${CSS}</style></head>
<body><main>
<header><h1>Flow test report <span class="pill ${report.verdict}">${escapeHtml(report.verdict)}</span></h1>
<p class="muted">${escapeHtml(report.finishedAt)} · ${ms(report.durationMs)}${report.env.goldens ? ' · goldens on' : ''}${theme} · ${link(report, report.paths.json, 'report.json')} · ${link(report, report.paths.debug, 'debug.jsonl')}</p></header>
<section class="stats">${stat(s.total, 'tests')}${stat(s.passed, 'passed', 'passed')}${stat(s.failed, 'failed', s.failed ? 'failed' : '')}${stat(s.skipped, 'skipped')}${stat(s.files, 'files')}${stat(ms(report.durationMs), 'total time')}</section>
${s.slowest.length ? `<h3>Slowest</h3><ol class="slowest">${s.slowest.map((t) => `<li>${ms(t.durationMs)} · ${escapeHtml(t.name)} <span class="muted">${escapeHtml(t.file)}</span></li>`).join('')}</ol>` : ''}
${report.errors.length ? `<h3>Run errors (${report.errors.length})</h3>${report.errors.map(errorBlock).join('')}` : ''}
${report.warnings.length ? `<h3>Run warnings (${report.warnings.length})</h3><pre class="warn">${escapeHtml(report.warnings.join('\n'))}</pre>` : ''}
${screenshotsSection(report)}
${ordered.map(([file, tests]) => `<section><h2>${escapeHtml(file)}</h2>${tests.map((t) => testBlock(report, t)).join('')}</section>`).join('\n')}
</main></body></html>
`;
}
