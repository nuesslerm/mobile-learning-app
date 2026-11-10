/**
 * The dark theme, which every other check has been blind to.
 *
 * Headless Chromium reports `prefers-color-scheme: light`, so the whole suite
 * renders a white page while the device runs dark. That is not a detail: four
 * defects shipped past seven green suites for exactly this reason, and all four
 * were the same shape -- a component that never sets its own colour, which is
 * fine on white and invisible on black:
 *
 *   - a sheet painted in the page's own `background` token
 *   - a wheel picker falling back to React Native's default black text
 *   - a dialog whose slot component discarded the caller's `style`, losing both
 *     the card surface and the backdrop dim
 *
 * None of them can reproduce on a light page. `withServedApp`'s `colorScheme`
 * option forces the shim, so they can now.
 *
 * What this asserts is CONTRAST, not exact colours: an elevated surface must
 * differ from the page behind it, and text must differ from the surface it sits
 * on. A token swap that keeps both readable should not fail this; a component
 * that inherits a light-surface default will.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const problems = [];
function check(name, ok, detail) {
  problems.push({ name, ok, detail });
  console.log(`${ok ? '  ok  ' : '  FAIL'} ${name}${ok ? '' : ` — ${detail}`}`);
}

/** Perceived brightness, so "these two are indistinguishable" is one number. */
const LUMA = `(rgb) => {
  const m = /rgba?\\(([^)]+)\\)/.exec(rgb || '');
  if (!m) return null;
  const [r, g, b] = m[1].split(',').map((n) => Number(n.trim()));
  return Math.round(0.2126 * r + 0.7152 * g + 0.0722 * b);
}`;

await withServedApp(
  {
    port: 8221,
    name: 'dark-theme',
    devicePreset: 'iphone-15-pro',
    colorScheme: 'dark',
  },
  async ({ container, name, evalInApp, tapInApp, goto }) => {
    await goto('');

    const page = await evalInApp(`(() => {
      const luma = ${LUMA};
      const body = appDoc.defaultView.getComputedStyle(appDoc.body);
      return { background: body.backgroundColor, luma: luma(body.backgroundColor) };
    })()`);

    // If this fails the shim did not take, and every assertion below would be
    // measuring the light theme while claiming to test the dark one.
    check(
      'the app is actually rendering dark',
      page?.luma !== null && page.luma < 128,
      `body background ${page?.background} (luma ${page?.luma}) — the colour-scheme shim did not take, so nothing below tests the dark theme`
    );
    if (!(page?.luma < 128)) {
await report(container, name);
      return;
    }

    await container.tap({
      name,
      ref: '[data-testid="LeetcodeListItem"]',
    });
    for (let i = 0; i < 20; i++) {
      await new Promise((r) => setTimeout(r, 400));
      if (
        await evalInApp(
          `!!appDoc.querySelector('[data-testid="ProblemEditorInfoButton"]')`
        )
      )
        break;
    }
    await tapInApp('[data-testid="ProblemEditorInfoButton"]');

    let sheet = null;
    for (let i = 0; i < 20; i++) {
      await new Promise((r) => setTimeout(r, 400));
      sheet = await evalInApp(`(() => {
        const luma = ${LUMA};
        const view = appDoc.defaultView;
        const el = appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]');
        if (!el) return null;
        // The sheet's own frame is transparent; the painted surface is the
        // nearest ancestor that actually sets a background.
        let surface = el, surfaceColor = 'rgba(0, 0, 0, 0)';
        for (let n = 0; n < 6 && surface; n++) {
          const c = view.getComputedStyle(surface).backgroundColor;
          if (c && !/rgba\\(0, 0, 0, 0\\)|transparent/.test(c)) { surfaceColor = c; break; }
          surface = surface.parentElement;
        }
        const title = appDoc.querySelector('[data-testid="ProblemDescriptionSheetHeader"] div, [data-testid="ProblemDescriptionSheetHeader"] span');
        return {
          surface: surfaceColor,
          surfaceLuma: luma(surfaceColor),
          text: title ? view.getComputedStyle(title).color : null,
          textLuma: title ? luma(view.getComputedStyle(title).color) : null,
        };
      })()`);
      if (sheet?.surfaceLuma !== null && sheet?.surfaceLuma !== undefined) break;
    }

    check(
      'the description sheet mounted',
      sheet !== null,
      'the sheet never rendered, so its colours cannot be measured'
    );

    if (sheet) {
      // The bug this exists for: `backgroundStyle` used the page's own
      // `background` token, so on dark the sheet was the same black as the page
      // and only its handle and one hairline read as a sheet at all.
      check(
        'the sheet surface is distinguishable from the page behind it',
        sheet.surfaceLuma !== null &&
          Math.abs(sheet.surfaceLuma - page.luma) >= 4,
        `sheet ${sheet.surface} (luma ${sheet.surfaceLuma}) vs page ${page.background} (luma ${page.luma}) — the sheet has no visible surface on dark`
      );
      // And the other half: text that inherits a light-surface default.
      check(
        'sheet text is readable against its own surface',
        sheet.textLuma !== null &&
          Math.abs(sheet.textLuma - sheet.surfaceLuma) >= 60,
        `text ${sheet.text} (luma ${sheet.textLuma}) on surface ${sheet.surface} (luma ${sheet.surfaceLuma}) — too close to read`
      );
    }

    // The language dial. Its rows are the other half of the same bug class:
    // the wheel applies its own fade on the item container, so a row can be
    // correctly coloured and still unreadable. Measure what reaches the eye --
    // the row colour composited over the surface at the container's opacity --
    // rather than the colour the row asked for.
    await tapInApp('[data-testid="ProblemDescriptionSheetHeaderClose"]');
    await new Promise((r) => setTimeout(r, 800));
    await tapInApp('[data-testid="ProblemEditorLanguagePicker-Trigger"]');

    let wheel = null;
    for (let i = 0; i < 20; i++) {
      await new Promise((r) => setTimeout(r, 400));
      wheel = await evalInApp(`(() => {
        const luma = ${LUMA};
        const view = appDoc.defaultView;
        const row = appDoc.querySelector('[data-testid="ProblemEditorLanguagePicker-JavaScript"]');
        if (!row) return null;
        let surface = row, surfaceColor = null, opacity = 1, n = row;
        for (let i = 0; i < 8 && n; i++) {
          const cs = view.getComputedStyle(n);
          const o = Number(cs.opacity);
          if (!Number.isNaN(o)) opacity *= o;
          if (!surfaceColor && i > 0 && cs.backgroundColor
              && !/rgba\\(0, 0, 0, 0\\)|transparent/.test(cs.backgroundColor)) {
            surfaceColor = cs.backgroundColor;
          }
          n = n.parentElement;
        }
        const color = view.getComputedStyle(row).color;
        return { color, opacity, surface: surfaceColor };
      })()`);
      if (wheel) break;
    }

    console.log(`  measured wheel row: ${JSON.stringify(wheel)}`);

    check(
      'the language dial mounted',
      wheel !== null,
      'the wheel never rendered, so its rows cannot be measured'
    );

    if (wheel) {
      // What the eye gets is the row blended toward its backdrop by the fade,
      // so the ratio is computed on the COMPOSITED colour, not the one the row
      // asked for. 4.5:1 is WCAG AA for body text; the library's own 0.2
      // neighbour opacity lands at ~3:1 against this sheet.
      const backdrop = wheel.surface ?? page.background;
      const ratio = contrastRatio(
        composite(wheel.color, backdrop, wheel.opacity),
        backdrop
      );
      check(
        'the UNSELECTED language row is readable',
        ratio !== null && ratio >= 4.5,
        `row ${wheel.color} at container opacity ${wheel.opacity.toFixed(2)} over ` +
          `${backdrop} reads at ${ratio?.toFixed(1)}:1 — below 4.5:1, so the ` +
          `other language cannot be read`
      );
    }

    // Readable is not the same as reachable. The debug FAB is pinned to the
    // bottom-right of the screen, which was empty while every sheet was
    // full-height; over a 32%-tall dial it lands on the second row.
    const occluder = await evalInApp(`(() => {
      const row = appDoc.querySelector('[data-testid="ProblemEditorLanguagePicker-JavaScript"]');
      const sheet = appDoc.querySelector('[data-testid="ProblemEditorLanguagePickerSheet"]');
      if (!row || !sheet) return null;
      const r = row.getBoundingClientRect();
      const y = r.top + r.height / 2;
      // Sample ACROSS the row, not just its centre: the FAB is pinned to the
      // right edge, so a centre-only probe reports clear while the row's own
      // right-hand third is untappable.
      const blocked = [];
      for (let i = 1; i <= 9; i++) {
        const x = r.left + (r.width * i) / 10;
        const hit = appDoc.elementFromPoint(x, y);
        if (hit && sheet.contains(hit)) continue;
        const named = hit && hit.closest('[data-testid]');
        blocked.push({ x: Math.round(x), by: named ? named.getAttribute('data-testid') : (hit ? hit.tagName : 'nothing') });
      }
      return { width: Math.round(r.width), blocked };
    })()`);

    console.log(`  measured occlusion: ${JSON.stringify(occluder)}`);

    check(
      'the unselected language row is not covered',
      occluder !== null && occluder.blocked.length === 0,
      `${occluder?.blocked.length}/9 sample points across the row land outside the ` +
        `sheet — on ${[...new Set((occluder?.blocked ?? []).map((b) => b.by))].join(', ')}`
    );

    await report(container, name);
  }
);

function channels(rgb) {
  const m = /rgba?\(([^)]+)\)/.exec(rgb || '');
  return m ? m[1].split(',').slice(0, 3).map((n) => Number(n.trim())) : null;
}

/** Source-over of `top` on `bottom` at `alpha`, as the fade actually paints it. */
function composite(top, bottom, alpha) {
  const t = channels(top);
  const b = channels(bottom);
  if (!t || !b) return null;
  const mix = (i) => Math.round(b[i] + (t[i] - b[i]) * alpha);
  return `rgb(${mix(0)}, ${mix(1)}, ${mix(2)})`;
}

/** WCAG 2.1 contrast ratio -- gamma-corrected, unlike the luma proxy above. */
function contrastRatio(a, b) {
  const rel = (rgb) => {
    const c = channels(rgb);
    if (!c) return null;
    const [r, g, bl] = c.map((v) => {
      const s = v / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const la = rel(a);
  const lb = rel(b);
  if (la === null || lb === null) return null;
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}


async function report(container, name) {
  const failed = problems.filter((p) => !p.ok);
  const shot = await container.screenshot({ name });
  if (shot?.path) console.log(`screenshot: ${shot.path}`);
  if (failed.length) {
    fail(
      `${failed.length}/${problems.length} dark-theme checks failed: ` +
        failed.map((f) => f.name).join('; ')
    );
    return;
  }
  console.log(`ok: all ${problems.length} dark-theme checks passed`);
}
