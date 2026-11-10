/**
 * The header overflow menu, driven for real.
 *
 * Worth a dedicated check because the two things most likely to break here are
 * both invisible to a typecheck. A navigator `header` renders OUTSIDE the app's
 * SafeAreaProvider subtree (which is why ScreenHeader reads the inset via
 * context rather than the hook that throws), and Tamagui's Popover portals its
 * content -- and this repo has previously lost a portalled overlay to
 * react-navigation's stacking contexts, which is why the keyboard toolbar
 * carries a hand-rolled web portal. So: open it, and assert the content is
 * really on screen and really clickable, not merely mounted.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

/**
 * The editor is a CodeMirror iframe, so its text is not in the app document --
 * every assertion about the buffer has to reach through the frame.
 */
async function editorText(evalInApp) {
  const text = await evalInApp(`(() => {
    const zone = appDoc.querySelector('[data-testid="ProblemEditorEditorZone"]');
    const frame = zone ? zone.querySelector('iframe') : null;
    if (!frame) return '';
    try {
      const content = frame.contentDocument && frame.contentDocument.querySelector('.cm-content');
      return content ? content.textContent : '';
    } catch (err) {
      return '';
    }
  })()`).catch(() => '');
  return typeof text === 'string' ? text : '';
}

await withServedApp(
  {
    port: 8207,
    name: 'header-menu-verify',
  },
  async ({ container, name, evalInApp, tapInApp, goto }) => {
    // Reached by tapping, not by deep link: `leetcode/[id]` is a dynamic route
    // and a static export has no file for a concrete id, so navigating there
    // directly serves the 404 page. The harness resolves the selector with
    // querySelector, so it taps the first of the 72 rows.
    await container.tap({ name, ref: '[data-testid="LeetcodeListItem"]' });

    let before = null;
    for (let attempt = 0; attempt < 16; attempt++) {
      await new Promise((r) => setTimeout(r, 500));
      try {
        before = await evalInApp(`(() => {
          const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
          return {
            header: !!q('ProblemEditorHeader'),
            info: !!q('ProblemEditorInfoButton'),
            menuButton: !!q('ProblemEditorMenuButton'),
            contentBeforeOpen: !!q('ProblemEditorMenuButtonContent'),
          };
        })()`);
      } catch (err) {
        // "Frame was detached" is what an eval that races the navigation looks
        // like -- it means the tap worked, not that the screen is broken.
        if (!/detached/i.test(String(err))) throw err;
      }
      if (before?.menuButton) break;
    }

    if (!before?.header || !before.info || !before.menuButton) {
      fail(`the header did not render its controls: ${JSON.stringify(before)}`);
      return;
    }
    // Already mounted before the tap would make "it opened" below vacuous.
    if (before.contentBeforeOpen) {
      fail('the menu content was already mounted before the trigger was tapped');
      return;
    }

    await tapInApp('[data-testid="ProblemEditorMenuButton"]');
    await new Promise((r) => setTimeout(r, 600));

    const opened = await evalInApp(`(() => {
      const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
      const content = q('ProblemEditorMenuButtonContent');
      if (!content) return { open: false };
      const r = content.getBoundingClientRect();
      const w = appDoc.defaultView;
      const rows = ['ProblemEditorMenuFormat','ProblemEditorMenuHistory','ProblemEditorMenuLogin']
        .map((id) => {
          const el = q(id);
          if (!el) return { id, present: false };
          const b = el.getBoundingClientRect();
          // elementFromPoint is the difference between "in the DOM" and "the
          // thing a finger would actually hit" -- a portal that lands behind
          // the screen passes every other check.
          const hit = appDoc.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
          return {
            id,
            present: true,
            text: el.textContent,
            onTop: !!hit && (hit === el || el.contains(hit)),
          };
        });
      return {
        open: true,
        rect: { top: Math.round(r.top), right: Math.round(r.right), w: Math.round(r.width), h: Math.round(r.height) },
        viewport: { w: w.innerWidth, h: w.innerHeight },
        rows,
      };
    })()`);

    if (!opened?.open) {
      fail('tapping the three-dots control did not mount the menu content');
      return;
    }
    if (!opened.rect.w || !opened.rect.h) {
      fail(`the menu mounted with no box: ${JSON.stringify(opened.rect)}`);
      return;
    }
    // On screen in both axes. Off-screen is the classic
    // portal-outside-the-stacking-context failure, and it is what this check
    // exists to catch whether the surface is a popover or a sheet.
    if (opened.rect.top < 0 || opened.rect.right > opened.viewport.w + 2) {
      fail(`the menu is positioned off-screen: ${JSON.stringify(opened)}`);
      return;
    }

    const missing = opened.rows.filter((r) => !r.present).map((r) => r.id);
    if (missing.length) {
      fail(`menu rows missing: ${missing.join(', ')}`);
      return;
    }
    const buried = opened.rows.filter((r) => !r.onTop).map((r) => r.id);
    if (buried.length) {
      fail(
        `menu rows are in the DOM but not hit-testable (something is painted over them): ${buried.join(', ')}`
      );
      return;
    }

    await container.screenshot({ name });

    // History is the row with a behaviour worth asserting: it navigates to its
    // own route and the sheet must actually come up.
    // tapInApp hit-tests and dispatches inside a single eval, so when the tap
    // navigates -- as this one is supposed to -- that eval's own return races
    // the teardown of the frame it ran in. A detach here means the tap landed.
    try {
      await tapInApp('[data-testid="ProblemEditorMenuHistory"]');
    } catch (err) {
      if (!/detached/i.test(String(err))) throw err;
    }
    await new Promise((r) => setTimeout(r, 900));

    // Same detach race as the first navigation: selecting the row pushes a
    // route, so the first eval after it can land on a frame that is already
    // going away.
    let history = null;
    for (let attempt = 0; attempt < 12; attempt++) {
      try {
        history = await evalInApp(`(() => {
          const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
          return {
            path: appDoc.defaultView.location.pathname,
            sheet: !!q('CodeHistorySheet'),
            empty: !!q('CodeHistoryEmpty'),
            rows: appDoc.querySelectorAll('[data-testid="CodeHistoryRow"]').length,
            menuStillOpen: !!q('ProblemEditorMenuButtonContent'),
          };
        })()`);
      } catch (err) {
        if (!/detached/i.test(String(err))) throw err;
      }
      if (history?.sheet) break;
      await new Promise((r) => setTimeout(r, 500));
    }

    if (!history?.sheet) {
      fail(`the history row did not open its sheet: ${JSON.stringify(history)}`);
      return;
    }
    // The menu must close on selection: a popover left mounted over a screen
    // that has already been replaced floats above the wrong route.
    if (history.menuStillOpen) {
      fail('the menu stayed open after a row was selected');
      return;
    }

    await container.screenshot({ name });

    // The empty state is the easy half. Now make history do its job: a
    // language switch discards the buffer, which is exactly the data loss the
    // store exists to undo, and unlike Format it needs no backend.
    // Back to the list via a real navigation rather than dismissing the sheet:
    // the list lives at the app root, and a static export needs the harness to
    // manage the frame swap (see `goto`'s note on history.back()).
    await goto('');
    await container.tap({ name, ref: '[data-testid="LeetcodeListItem"]' });
    await new Promise((r) => setTimeout(r, 3000));

    const pythonText = await editorText(evalInApp);
    if (!pythonText.length) {
      fail('the editor never loaded its starter code, so there is nothing to lose');
      return;
    }

    // The picker is a wheel in a sheet now -- open it first. The per-language
    // testID this taps is deliberately unchanged.
    await tapInApp('[data-testid="ProblemEditorLanguagePicker-Trigger"]');
    await new Promise((r) => setTimeout(r, 900));
    await tapInApp('[data-testid="ProblemEditorLanguagePicker-JavaScript"]');
    await new Promise((r) => setTimeout(r, 1200));

    const jsText = await editorText(evalInApp);
    if (jsText === pythonText) {
      fail('the language switch did not replace the buffer, so nothing was at risk');
      return;
    }

    await tapInApp('[data-testid="ProblemEditorMenuButton"]');
    await new Promise((r) => setTimeout(r, 600));
    try {
      await tapInApp('[data-testid="ProblemEditorMenuHistory"]');
    } catch (err) {
      if (!/detached/i.test(String(err))) throw err;
    }

    let saved = null;
    for (let attempt = 0; attempt < 12; attempt++) {
      try {
        saved = await evalInApp(`(() => {
          const rows = [...appDoc.querySelectorAll('[data-testid="CodeHistoryRow"]')];
          return {
            rows: rows.length,
            reasons: [...appDoc.querySelectorAll('[data-testid="CodeHistoryRowReason"]')].map((e) => e.textContent),
          };
        })()`);
      } catch (err) {
        if (!/detached/i.test(String(err))) throw err;
      }
      if (saved?.rows) break;
      await new Promise((r) => setTimeout(r, 500));
    }

    if (!saved?.rows) {
      fail(`the language switch did not produce a version: ${JSON.stringify(saved)}`);
      return;
    }

    await container.screenshot({ name });

    // Restore has to put the ORIGINAL buffer back. Asserting only that a row
    // exists would pass with a Restore button wired to nothing.
    try {
      await tapInApp('[data-testid="CodeHistoryRestore"]');
    } catch (err) {
      if (!/detached/i.test(String(err))) throw err;
    }
    // Restoring closes the sheet route and writes through the editor ref, so
    // the buffer is only readable once the editor iframe is back on top. A
    // fixed sleep here reads 0 chars and looks like a broken Restore.
    let restored = '';
    for (let attempt = 0; attempt < 16; attempt++) {
      await new Promise((r) => setTimeout(r, 500));
      restored = await editorText(evalInApp);
      if (restored === pythonText) break;
    }

    if (restored !== pythonText) {
      fail(
        `Restore did not bring the pre-switch buffer back — expected ${pythonText.length} chars, got ${restored.length}`
      );
      return;
    }

    const rowSummary = opened.rows.map((r) => `${r.id.replace('ProblemEditorMenu', '')}="${r.text}"`).join(', ');
    console.log(
      `ok: language switch saved a version (${saved.rows} row, "${saved.reasons[0]}") and Restore ` +
        `brought back all ${restored.length} chars of the pre-switch buffer`
    );
    console.log(
      `ok: menu opened at top ${opened.rect.top}, right ${opened.rect.right} of ${opened.viewport.w}px — ` +
        `3 rows all hit-testable (${rowSummary}); ` +
        `History opened ${history.path} with ${history.rows} rows${history.empty ? ' (empty state)' : ''}, menu closed`
    );
  }
);
