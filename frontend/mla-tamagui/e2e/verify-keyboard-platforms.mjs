/**
 * The mock keyboard must reshape the screen the way the emulated platform
 * really does, because the two strategies hide different bugs:
 *
 *  - Android RESIZES: the layout viewport shrinks by the keyboard's height, so
 *    a bottom-anchored bar rides up on its own even if the app does nothing.
 *  - iOS OVERLAYS: the viewport keeps its height and the keyboard is drawn on
 *    top, so a bar pinned to the bottom with no safe-area awareness stays
 *    COVERED. An app that looks correct under resize can be broken here.
 *
 * The harness shrank the iframe on every device before this check existed,
 * which meant an iPhone frame was quietly showing Android behaviour -- so the
 * one failure mode the device frame is meant to expose could not appear.
 *
 * Asserted on the editor screen, whose toolbar rail is exactly such a
 * bottom-anchored bar.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const MEASURE = `(() => {
  const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
  const rail = q('EditorHelperRails');
  const view = appDoc.defaultView;
  return {
    innerHeight: view.innerHeight,
    mode: view.__MCP_KEYBOARD_MODE__ ?? null,
    reportedHeight: view.__MCP_KEYBOARD_HEIGHT__ ?? null,
    railTop: rail ? Math.round(rail.getBoundingClientRect().top) : null,
    railBottom: rail ? Math.round(rail.getBoundingClientRect().bottom) : null,
  };
})()`;

async function openEditorAndFocus({ container, name, evalInApp }) {
  await container.tap({ name, ref: '[data-testid="LeetcodeListItem"]' });
  for (let attempt = 0; attempt < 16; attempt++) {
    await new Promise((r) => setTimeout(r, 500));
    const ready = await evalInApp(`(() => {
      const zone = appDoc.querySelector('[data-testid="ProblemEditorEditorZone"]');
      const frame = zone ? zone.querySelector('iframe') : null;
      return !!(frame && frame.contentDocument && frame.contentDocument.querySelector('.cm-content'));
    })()`);
    if (ready) break;
  }
  // Focusing the editor is what raises the mock keyboard: the daemon watches
  // focus and toggles the shell, so tapping is the whole trigger.
  await container.tap({ name, ref: '.cm-content' });
  for (let attempt = 0; attempt < 12; attempt++) {
    await new Promise((r) => setTimeout(r, 400));
    const reading = await evalInApp(MEASURE);
    if (reading?.railTop !== null) return reading;
  }
  return evalInApp(MEASURE);
}

const device = process.argv[2] ?? 'iphone-15-pro';
const expected = device === 'pixel-8' ? 'resize' : 'overlay';

await withServedApp(
  { port: 8213, name: `keyboard-${device}`, devicePreset: device },
  async (ctx) => {
    const reading = await openEditorAndFocus(ctx);

    if (!reading?.railTop && reading?.railTop !== 0) {
      fail(`the keyboard toolbar rail never mounted: ${JSON.stringify(reading)}`);
      return;
    }
    if (reading.mode !== expected) {
      fail(`${device} reported keyboard mode "${reading.mode}", expected "${expected}"`);
      return;
    }

    if (expected === 'resize') {
      // The viewport itself shrank, so the rail sits at the very bottom of what
      // is left of the screen. Nothing the app does is involved.
      if (Math.abs(reading.railBottom - reading.innerHeight) > 8) {
        fail(
          `under resize the rail should sit flush at the shrunken viewport's bottom, ` +
            `but rail ends at ${reading.railBottom} in a ${reading.innerHeight}px viewport: ` +
            JSON.stringify(reading)
        );
        return;
      }
      if (reading.reportedHeight !== 0) {
        fail(
          `under resize the app must NOT be told a keyboard height -- the shrunken ` +
            `viewport already carries it, and adding it double-shifts the layout: ` +
            JSON.stringify(reading)
        );
        return;
      }
    } else {
      // The viewport kept its height, so the rail has to be lifted by the app
      // using the inset the harness reports. If it were still flush with the
      // viewport bottom it would be sitting under the keyboard.
      const liftedBy = reading.innerHeight - reading.railBottom;
      if (reading.reportedHeight !== 270) {
        fail(`under overlay the harness must report the covered height, got ${reading.reportedHeight}`);
        return;
      }
      if (liftedBy < 200) {
        fail(
          `under overlay the rail is only ${liftedBy}px off the bottom of a ` +
            `${reading.innerHeight}px viewport, so it is underneath the keyboard: ` +
            JSON.stringify(reading)
        );
        return;
      }
    }

    // Typing parity: press a real key ON the mock keyboard and confirm the
    // character reaches CodeMirror. The keyboard's buttons live in the shell
    // page, so this is the one thing that exercises the whole path --
    // pointerdown -> window.__shell.sendKey -> the daemon's key poll ->
    // page.keyboard.insertText -> the focused contenteditable. Asserting the
    // rail's position alone would pass with the keys wired to nothing.
    const before = await ctx.evalInApp(`(() => {
      const frame = appDoc.querySelector('[data-testid="ProblemEditorEditorZone"] iframe');
      const cm = frame && frame.contentDocument && frame.contentDocument.querySelector('.cm-content');
      return cm ? cm.textContent.length : -1;
    })()`);

    await ctx.container.shellEval({
      name: `keyboard-${device}`,
      // onPointerDown, not click: the keyboard fires on pointerdown so the
      // press registers before the focused element can blur.
      expression: `(() => {
        const key = document.querySelector('[data-mcp-key="q"]');
        if (!key) return 'no key button';
        key.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, pointerId: 1, isPrimary: true }));
        return 'pressed';
      })()`,
    });

    let after = before;
    for (let attempt = 0; attempt < 12; attempt++) {
      await new Promise((r) => setTimeout(r, 400));
      after = await ctx.evalInApp(`(() => {
        const frame = appDoc.querySelector('[data-testid="ProblemEditorEditorZone"] iframe');
        const cm = frame && frame.contentDocument && frame.contentDocument.querySelector('.cm-content');
        return cm ? cm.textContent.length : -1;
      })()`);
      if (after > before) break;
    }
    if (!(after > before)) {
      fail(
        `pressing a mock-keyboard key under ${expected} did not reach the editor ` +
          `(document stayed at ${before} chars), so keyboard input is broken in this mode`
      );
      return;
    }

    // The rail must be ON TOP of the debug FAB. Both are absolutely positioned
    // over the bottom of the screen, and the FAB renders after the navigator,
    // so at an equal z-index it wins on paint order and swallows the rail's
    // right-hand keys -- Rename and Done, the end of the row worth reaching.
    // Asserted by hit test rather than by reading z-index, since what matters
    // is which element actually receives the tap.
    const overlap = await ctx.evalInApp(`(() => {
      const fab = appDoc.querySelector('[data-testid="DebugFab"]');
      const rail = appDoc.querySelector('[data-testid="EditorHelperRails"]');
      if (!fab || !rail) return { checked: false, reason: 'FAB or rail missing' };
      const f = fab.getBoundingClientRect();
      const r = rail.getBoundingClientRect();
      const intersects = f.left < r.right && f.right > r.left && f.top < r.bottom && f.bottom > r.top;
      if (!intersects) return { checked: false, reason: 'they do not overlap on this device' };
      const x = Math.max(f.left, r.left) + Math.min(f.width, r.width) / 2;
      const y = Math.max(f.top, r.top) + 2;
      const hit = appDoc.elementFromPoint(x, y);
      return {
        checked: true,
        railOnTop: !!hit && (rail === hit || rail.contains(hit)),
        hit: hit ? (hit.getAttribute('data-testid') || hit.tagName) : null,
        fabIsAncestorOfHit: !!hit && fab.contains(hit),
      };
    })()`);
    if (overlap?.checked && !overlap.railOnTop) {
      fail(
        `the debug FAB is painted over the keyboard toolbar -- a tap where they ` +
          `overlap lands on ${JSON.stringify(overlap.hit)} instead of the rail: ` +
          JSON.stringify(overlap)
      );
      return;
    }

    const shot = await ctx.container.screenshot({ name: `keyboard-${device}` });
    console.log(
      `ok: ${device} → ${reading.mode}; viewport ${reading.innerHeight}px, ` +
        `reported inset ${reading.reportedHeight}px, rail ${reading.railTop}–${reading.railBottom}; ` +
        `a mock-keyboard key reached the editor (${before} → ${after} chars); ` +
        `FAB/rail overlap ${overlap?.checked ? (overlap.railOnTop ? 'rail on top' : 'BAD') : overlap?.reason}`
    );
    if (shot?.path) console.log(`screenshot: ${shot.path}`);
  }
);
