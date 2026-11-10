/**
 * Turns the captured screenshots into one self-contained HTML report.
 *
 * Images are inlined as data URIs so the page is a single file that keeps
 * working after the run that produced it is gone -- a report whose <img> tags
 * point at a temp directory is a report that is broken by the time anyone
 * opens it.
 */
import { Buffer } from 'node:buffer';
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const REPORT_DIR = join(APP_DIR, '.report');
const OUT = join(REPORT_DIR, 'index.html');

const manifests = readdirSync(REPORT_DIR)
  .filter((f) => f.endsWith('.json'))
  .map((f) => JSON.parse(readFileSync(join(REPORT_DIR, f), 'utf8')));

if (manifests.length === 0) {
  console.error('FAIL: no manifests in .report — run `pnpm run report:capture` first');
  process.exit(1);
}

const dataUri = (file) =>
  `data:image/png;base64,${readFileSync(join(REPORT_DIR, file)).toString('base64')}`;

const esc = (s) =>
  String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

const DEVICE_LABEL = { 'iphone-15-pro': 'iPhone 15 Pro', 'pixel-8': 'Pixel 8' };
const SCHEME_LABEL = { light: 'Light', dark: 'Dark' };

// One step = one row: the same moment across every device x scheme combo,
// side by side, so the platform and theme differences are visible rather than
// described. Each manifest is one (device, scheme) run, so index alignment
// across manifests is exactly the device x scheme grouping.
const steps = manifests[0].shots.map((shot, i) => ({
  title: shot.title,
  note: shot.note,
  frames: manifests.map((m) => ({
    device: m.device,
    scheme: m.scheme,
    shot: m.shots[i],
  })),
}));

const frameCount = manifests.reduce((n, m) => n + m.shots.length, 0);

const html = `<title>Blind 75 Contact Sheet</title>
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Archivo:wght@500;600;700&family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans:wght@400;500&display=swap">
<style>
  /* Light is the complete palette; the two blocks below redefine ONLY tokens,
     so an un-stamped (system-default) document still resolves every colour. */
  :root {
    --paper:#f4f6f8; --well:#e9edf2; --card:#ffffff;
    --ink:#14181d; --muted:#5c6672; --faint:#8b95a3;
    --line:#dbe1e9; --hair:#c7cfda;
    --mark:#e0592a;
    --shadow:0 1px 2px rgba(20,24,29,.06), 0 8px 24px rgba(20,24,29,.06);
  }
  @media (prefers-color-scheme: dark) {
    :root:not([data-theme="light"]) {
      --paper:#0f1216; --well:#161b21; --card:#12161b;
      --ink:#e7ecf2; --muted:#98a3b1; --faint:#6b7684;
      --line:#232a33; --hair:#2e3742;
      --mark:#ff7a4d;
      --shadow:0 1px 2px rgba(0,0,0,.4), 0 10px 28px rgba(0,0,0,.35);
    }
  }
  :root[data-theme="dark"] {
    --paper:#0f1216; --well:#161b21; --card:#12161b;
    --ink:#e7ecf2; --muted:#98a3b1; --faint:#6b7684;
    --line:#232a33; --hair:#2e3742;
    --mark:#ff7a4d;
    --shadow:0 1px 2px rgba(0,0,0,.4), 0 10px 28px rgba(0,0,0,.35);
  }

  * { box-sizing: border-box; }
  body {
    margin:0; background:var(--paper); color:var(--ink);
    font-family:"IBM Plex Sans", ui-sans-serif, system-ui, -apple-system, sans-serif;
    font-size:15px; line-height:1.65; -webkit-font-smoothing:antialiased;
  }
  .wrap { max-width:1140px; margin:0 auto; padding:56px 28px 110px; }

  .eyebrow {
    font-family:"IBM Plex Mono", ui-monospace, Menlo, monospace;
    font-size:11.5px; letter-spacing:.16em; text-transform:uppercase;
    color:var(--mark); margin:0 0 14px;
  }
  h1 {
    font-family:Archivo, ui-sans-serif, system-ui, sans-serif;
    font-weight:700; font-size:clamp(30px, 4.4vw, 46px); line-height:1.05;
    letter-spacing:-.025em; margin:0 0 16px; text-wrap:balance; max-width:16ch;
  }
  .lede { color:var(--muted); margin:0; max-width:64ch; font-size:16.5px; }

  .facts {
    display:grid; grid-template-columns:repeat(auto-fit, minmax(150px,1fr));
    gap:1px; background:var(--line); border:1px solid var(--line);
    border-radius:10px; overflow:hidden; margin:36px 0 0;
  }
  .fact { background:var(--card); padding:14px 16px; }
  .fact dt {
    font-family:"IBM Plex Mono", ui-monospace, Menlo, monospace;
    font-size:10.5px; letter-spacing:.13em; text-transform:uppercase;
    color:var(--faint); margin:0 0 4px;
  }
  .fact dd {
    margin:0; font-family:Archivo, ui-sans-serif, sans-serif; font-weight:600;
    font-size:17px; font-variant-numeric:tabular-nums; letter-spacing:-.01em;
  }
  .fact dd small { font-weight:500; color:var(--muted); font-size:13px; }

  .steps { margin-top:56px; }
  .step {
    display:grid; grid-template-columns:56px 1fr; gap:0 24px;
    padding-bottom:52px;
  }
  .spine { position:relative; }
  .spine .n {
    font-family:"IBM Plex Mono", ui-monospace, Menlo, monospace;
    font-size:13px; font-weight:500; color:var(--mark);
    font-variant-numeric:tabular-nums; display:block; padding-top:2px;
  }
  /* The rule runs the height of the step: the runbook is an ordered walk, so
     the sequence is real information rather than decoration. */
  .spine::after {
    content:""; position:absolute; left:5px; top:26px; bottom:-52px;
    width:1px; background:var(--hair);
  }
  .step:last-of-type .spine::after { display:none; }

  h2 {
    font-family:Archivo, ui-sans-serif, sans-serif; font-weight:600;
    font-size:21px; letter-spacing:-.015em; margin:0 0 8px; text-wrap:balance;
  }
  .note { color:var(--muted); margin:0 0 24px; max-width:70ch; }

  .frames { display:grid; grid-template-columns:repeat(auto-fit, minmax(250px,1fr)); gap:22px; }
  figure { margin:0; }
  figcaption {
    font-family:"IBM Plex Mono", ui-monospace, Menlo, monospace;
    font-size:11px; letter-spacing:.11em; text-transform:uppercase;
    color:var(--faint); margin:0 0 10px;
    display:flex; align-items:baseline; gap:8px;
  }
  figcaption .tag { color:var(--ink); }
  figcaption .scheme { color:var(--mark); }
  .frames-head {
    display:grid; grid-template-columns:repeat(auto-fit, minmax(250px,1fr)); gap:22px;
    margin:0 0 10px; font-family:"IBM Plex Mono", ui-monospace, Menlo, monospace;
    font-size:10.5px; letter-spacing:.13em; text-transform:uppercase; color:var(--faint);
  }
  img {
    display:block; width:100%; max-width:100%; border-radius:14px;
    background:var(--well); border:1px solid var(--line); box-shadow:var(--shadow);
  }

  footer {
    margin-top:24px; padding-top:34px; border-top:1px solid var(--line);
    color:var(--muted); font-size:14px; max-width:70ch;
  }
  footer h3 {
    font-family:Archivo, ui-sans-serif, sans-serif; font-size:15px;
    font-weight:600; color:var(--ink); margin:0 0 6px; letter-spacing:-.01em;
  }
  footer p { margin:0 0 20px; }
  code {
    font-family:"IBM Plex Mono", ui-monospace, Menlo, monospace;
    font-size:12.5px; background:var(--well); border:1px solid var(--line);
    padding:1px 5px; border-radius:5px;
  }
  @media (max-width:560px) {
    .step { grid-template-columns:1fr; gap:0; }
    .spine::after { display:none; }
    .spine .n { padding-bottom:6px; }
  }
</style>
<div class="wrap">
  <p class="eyebrow">mla-tamagui · runbook walkthrough</p>
  <h1>Every step, on both platforms</h1>
  <p class="lede">
    ${steps.length} steps of <code>docs/runbooks/mla-tamagui-sanity.md</code> plus the
    coding-agent walk, captured through the <code>wf browser</code> device frame with its
    mock keyboard. Each step shows the same moment across every
    <code>device × scheme</code> combination &mdash; so the keyboard difference between the two
    platforms and the theme difference between light and dark are things you can see rather than
    take on trust.
  </p>

  <dl class="facts">
    <div class="fact"><dt>Steps</dt><dd>${steps.length}</dd></div>
    <div class="fact"><dt>Screenshots</dt><dd>${frameCount}</dd></div>
    <div class="fact"><dt>Devices</dt><dd>${new Set(manifests.map((m) => m.device)).size} <small>iOS + Android</small></dd></div>
    <div class="fact"><dt>Schemes</dt><dd>${new Set(manifests.map((m) => m.scheme)).size} <small>light + dark</small></dd></div>
    <div class="fact"><dt>Captured</dt><dd><small>${new Date().toISOString().slice(0, 16).replace('T', ' ')} UTC</small></dd></div>
  </dl>

  <div class="steps">
  ${steps
    .map(
      (step, i) => `<section class="step">
    <div class="spine"><span class="n">${String(i + 1).padStart(2, '0')}</span></div>
    <div>
      <h2>${esc(step.title.replace(/ \(.*\)$/, ''))}</h2>
      <p class="note">${esc(step.note)}</p>
      <div class="frames">
        <div class="frames-head">
          ${step.frames
            .map((f) => {
              const deviceLabel = DEVICE_LABEL[f.device] ?? f.device;
              const schemeLabel = SCHEME_LABEL[f.scheme] ?? f.scheme;
              return `<span>${esc(deviceLabel)} · ${esc(schemeLabel)}</span>`;
            })
            .join('')}
        </div>
        ${step.frames
          .map((f) => {
            const variant = /\((.*)\)$/.exec(f.shot.title);
            const deviceLabel = DEVICE_LABEL[f.device] ?? f.device;
            const schemeLabel = SCHEME_LABEL[f.scheme] ?? f.scheme;
            return `<figure>
          <figcaption>
            <span class="tag device">${esc(deviceLabel)}</span>
            <span class="tag scheme" data-scheme="${esc(f.scheme)}">${esc(schemeLabel)}</span>${
              variant ? `<span>${esc(variant[1])}</span>` : ''
            }
          </figcaption>
          <img alt="${esc(f.shot.title)} on ${esc(deviceLabel)} (${esc(schemeLabel)})" src="${dataUri(f.shot.file)}">
        </figure>`;
          })
          .join('\n        ')}
      </div>
    </div>
  </section>`
    )
    .join('\n  ')}
  </div>

  <footer>
    <h3>What this is</h3>
    <p>
      The visual half of acceptance. The numeric half is
      <code>pnpm --filter mla-tamagui run test:e2e</code>, which walks the same flow and asserts on
      computed styles, element geometry and CodeMirror&rsquo;s own document &mdash; then throws the
      pixels away. Neither half replaces the other: an assertion cannot tell you a sheet looks
      wrong, and a screenshot cannot tell you a theme token resolved.
    </p>
    <h3>What it is not</h3>
    <p>
      Expo Go on a physical device. These frames are React Native Web in headless Chromium with an
      emulated keyboard &mdash; not UIKit. That gap, and why it cannot be closed from this
      environment, is recorded in <code>docs/known-gaps.md</code>.
    </p>
  </footer>
</div>`;

writeFileSync(OUT, html);
const kb = Math.round(Buffer.byteLength(html) / 1024);
console.log(`ok: ${steps.length} steps × ${frameCount} frames (${manifests.length} device×scheme) → ${OUT} (${kb} KB)`);
