/**
 * The shell's Light/Dark picker, driven the way a person drives it.
 *
 * Distinct from `verify-dark-theme.mjs`, which opens the browser in dark and
 * asserts what the app renders. This asserts the CONTROL: that changing the
 * select in the device-frame chrome actually re-emulates `prefers-color-scheme`
 * and repaints the app inside the frame.
 *
 * That is not a formality. `prefers-color-scheme` cannot be set from page JS,
 * so the picker has no local effect at all -- it round-trips to the daemon
 * through an exposed binding, and every link in that chain (select -> shell
 * state -> binding -> `page.emulateMedia`) fails silently. A picker that moves
 * and changes nothing looks exactly like one that works.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const problems = [];
function check(name, ok, detail) {
  problems.push({ name, ok, detail });
  console.log(`${ok ? '  ok  ' : '  FAIL'} ${name}${ok ? '' : ` — ${detail}`}`);
}

const LUMA = `(rgb) => {
  const m = /rgba?\\(([^)]+)\\)/.exec(rgb || '');
  if (!m) return null;
  const [r, g, b] = m[1].split(',').map((n) => Number(n.trim()));
  return Math.round(0.2126 * r + 0.7152 * g + 0.0722 * b);
}`;

const readAppLuma = `(() => {
  const luma = ${LUMA};
  const cs = appDoc.defaultView.getComputedStyle(appDoc.body);
  return { background: cs.backgroundColor, luma: luma(cs.backgroundColor) };
})()`;

/** Sets the picker the way a person would: pick the option, fire `change`. */
const pickScheme = (value) => `(() => {
  const select = document.querySelector('[data-testid="mcp-device-picker"] select[aria-label="Color scheme"]');
  if (!select) return { ok: false, reason: 'the color-scheme select is not in the shell chrome' };
  const option = [...select.options].find((o) => o.value === ${JSON.stringify(value)});
  if (!option) return { ok: false, reason: 'no "' + ${JSON.stringify(value)} + '" option' };
  select.value = option.value;
  select.dispatchEvent(new Event('change', { bubbles: true }));
  return { ok: true, options: [...select.options].map((o) => o.value) };
})()`;

await withServedApp(
  { port: 8223, name: 'scheme-picker', devicePreset: 'iphone-15-pro' },
  async ({ container, name, evalInApp, goto }) => {
    await goto('');

    const shellRead = async (expression) => {
      const { result } = await container.shellEval({ name, expression });
      return typeof result === 'string' ? JSON.parse(result) : result;
    };

    // The precondition. Without it "the app went dark" could just mean it was
    // dark all along, and the picker would never be exercised.
    const before = await evalInApp(readAppLuma);
    check(
      'the app starts light',
      before?.luma !== null && before.luma >= 128,
      `body ${before?.background} (luma ${before?.luma}) — it was not light to begin with, so switching to dark proves nothing`
    );

    const picked = await shellRead(pickScheme('dark'));
    check(
      'the shell chrome offers a color-scheme picker',
      picked?.ok === true,
      picked?.reason ?? 'the picker could not be operated'
    );

    let after = null;
    for (let i = 0; i < 15; i++) {
      await new Promise((r) => setTimeout(r, 400));
      after = await evalInApp(readAppLuma);
      if (after?.luma !== null && after.luma < 128) break;
    }

    check(
      'picking Dark repaints the app dark',
      after?.luma !== null && after.luma < 128,
      `body went from luma ${before?.luma} to ${after?.luma} (${after?.background}) — ` +
        `the select moved but prefers-color-scheme was never re-emulated`
    );

    const darkShot = await container.screenshot({ name });
    if (darkShot?.path) console.log(`screenshot (dark): ${darkShot.path}`);

    // And back, so the control is shown to be a control rather than a one-way
    // switch that happens to end in the state being asserted.
    await shellRead(pickScheme('light'));
    let restored = null;
    for (let i = 0; i < 15; i++) {
      await new Promise((r) => setTimeout(r, 400));
      restored = await evalInApp(readAppLuma);
      if (restored?.luma !== null && restored.luma >= 128) break;
    }
    check(
      'picking Light puts it back',
      restored?.luma !== null && restored.luma >= 128,
      `body stayed at luma ${restored?.luma} (${restored?.background}) — the picker only goes one way`
    );

    const lightShot = await container.screenshot({ name });
    if (lightShot?.path) console.log(`screenshot (light): ${lightShot.path}`);

    const failed = problems.filter((p) => !p.ok);
    if (failed.length) {
      fail(
        `${failed.length}/${problems.length} color-scheme picker checks failed: ` +
          failed.map((f) => f.name).join('; ')
      );
      return;
    }
    console.log(`ok: all ${problems.length} color-scheme picker checks passed`);
  }
);
