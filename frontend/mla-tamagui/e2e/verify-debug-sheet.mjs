/**
 * Phase-7 acceptance (#215): the debug FAB opens a sheet showing live logs.
 *
 * Three things fail silently here and each looks like the others: the FAB not
 * being mounted, the sheet not opening, and the log pipeline being wired but
 * empty. All three are asserted, and the boot line's text is read back -- an
 * empty sheet with a working FAB would otherwise pass as "the sheet opened".
 *
 * Openness is measured as GEOMETRY, not presence. Tamagui keeps the sheet
 * frame mounted and simply parks it below the viewport when closed, so
 * `querySelector(...)` is truthy either way, and so is a query for the log
 * rows inside it. Measured: top 877 in an 860px viewport when closed, 146 when
 * open. A presence check would pass with the FAB doing nothing at all.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const SHEET = '[data-testid="DebugLogSheet"]';

const MEASURE = `(() => {
  const el = appDoc.querySelector('${SHEET}');
  if (!el) return { present: false };
  const view = appDoc.defaultView;
  const r = el.getBoundingClientRect();
  const rows = [...el.querySelectorAll('[data-testid="DebugLogText"]')];
  const count = el.querySelector('[data-testid="DebugLogSheetCount"]');
  return {
    present: true,
    // "On screen" means the frame's top edge is actually inside the viewport.
    onScreen: r.top < view.innerHeight - 40,
    top: Math.round(r.top),
    viewportHeight: view.innerHeight,
    rowCount: rows.length,
    countLabel: count ? count.textContent : null,
    rows: rows.map((e) => e.textContent),
    levels: [...el.querySelectorAll('[data-testid="DebugLogLevel"]')].map((e) => e.textContent).slice(0, 5),
  };
})()`;

await withServedApp({ port: 8201, name: 'debug-sheet-verify', path: 'showcase' }, async ({ container, name, evalInApp, tapInApp }) => {
    const fabPresent = await evalInApp(`!!appDoc.querySelector('[data-testid="DebugFab"]')`);
    if (!fabPresent) {
      fail('the debug FAB is not mounted');
      return;
    }

    // Press a design-system Button first. Its press handler goes through
    // `Logger.info`, which is a DIFFERENT path into the tail than the boot
    // line's direct `liveTail.add` -- and it is the path that silently no-ops
    // when `configureLogger` was never called. Only asserting on the boot line
    // would pass with the whole LogLayer pipeline unwired.
    await tapInApp('[data-testid="ButtonDefault"]');

    const before = await evalInApp(MEASURE);
    if (before?.onScreen) {
      fail(`the log sheet was already on screen before the FAB was tapped: ${JSON.stringify(before)}`);
      return;
    }

    // The FAB answers to a RNGH tap. The SDK's container.tap lands off-target
    // in the scaled device frame, and tapInApp's hit test rejects the FAB for
    // the same scale mismatch -- so drive the gesture by dispatching the
    // pointer sequence on the FAB's wrap element (verified: opens the sheet).
    await evalInApp(`(() => {
      const wrap = appDoc.querySelector('[data-testid="DebugFabWrap"]');
      const r = wrap.getBoundingClientRect();
      const x = r.left + r.width / 2;
      const y = r.top + r.height / 2;
      const view = appDoc.defaultView;
      for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
        wrap.dispatchEvent(
          new view.PointerEvent(type, { bubbles: true, cancelable: true, view, button: 0, clientX: x, clientY: y, pointerId: 1, isPrimary: true })
        );
      }
      return true;
    })()`);

    // Every sink in @lib/logging is buffered (~1s), so a fixed sleep here is a
    // coin flip between "not logged" and "not flushed yet". Poll instead, and
    // let the assertions below report whatever the last reading actually was.
    // The poll also waits for the buffered button-press line: the sheet can be
    // on screen with the boot row before that flush lands.
    const pressLogged = (m) => !!m?.rows?.some((r) => r.includes('ds.button.press'));
    let after = await evalInApp(MEASURE);
    for (let attempt = 0; attempt < 12 && !(after?.onScreen && after.rowCount >= 2 && pressLogged(after)); attempt++) {
      await new Promise((r) => setTimeout(r, 500));
      after = await evalInApp(MEASURE);
    }
    if (!after?.onScreen) {
      fail(`the FAB did not bring the sheet on screen: ${JSON.stringify(after)}`);
      return;
    }
    if (!after.rowCount) {
      fail(`the sheet opened but rendered no log rows: ${JSON.stringify(after)}`);
      return;
    }
    // The boot line is emitted by lib/logger.ts. Its absence means the logger
    // module was never imported -- exactly the silent no-op app #1 hit.
    if (!after.rows.some((r) => r.includes('app.boot'))) {
      fail(`no app.boot line in the tail, so the logger is not live: ${JSON.stringify(after.rows)}`);
      return;
    }
    if (!after.rows.some((r) => r.includes('ds.button.press'))) {
      fail(
        `the button press never reached the tail, so Logger is unconfigured and every ` +
          `Logger.* call in the design system is a no-op: ${JSON.stringify(after.rows)}`
      );
      return;
    }

    const shot = await container.screenshot({ name });
    console.log(
      `ok: FAB moved the sheet from top ${before.present ? before.top : 'unmounted'} to ${after.top} (viewport ${after.viewportHeight}) — ` +
        `${after.rowCount} rows ("${after.countLabel}"), levels ${after.levels.join(',')}, ${JSON.stringify(after.rows)}`
    );
    if (shot?.path) console.log(`screenshot: ${shot.path}`);
});
