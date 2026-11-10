/**
 * Phase-8 acceptance (#216): the ported leetcode feature actually works.
 *
 * Drives the real flow rather than asserting files exist: the problem list
 * renders from the catalog, a row navigates to the editor, and the CodeMirror
 * WebView inside it accepts input. The editor is the part most likely to
 * bundle cleanly and still be dead -- it is an iframe on web, so "the screen
 * rendered" says nothing about whether the editor came up.
 *
 * Every app-level assertion goes through `evalInApp`, never a bare `eval`: the
 * SDK targets the deepest editable frame so keystrokes reach CodeMirror, which
 * means once the editor mounts a plain `document.querySelector` is looking
 * inside CodeMirror's srcdoc iframe and returns null for everything.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

await withServedApp(
  { port: 8203, name: 'leetcode-verify' },
  async ({ container, name, evalInApp, tapInApp }) => {
    const list = await evalInApp(`(() => {
      const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
      const rows = [...appDoc.querySelectorAll('[data-testid="LeetcodeListItem"]')];
      const titles = [...appDoc.querySelectorAll('[data-testid="LeetcodeListItemTitle"]')].map((e) => e.textContent);
      const sections = [...appDoc.querySelectorAll('[data-testid="LeetcodeListSectionTitle"]')].map((e) => e.textContent);
      return {
        hero: q('LeetcodeListHeroTitle') ? q('LeetcodeListHeroTitle').textContent : null,
        heroCount: q('LeetcodeListHeroCountText') ? q('LeetcodeListHeroCountText').textContent : null,
        authRow: !!q('LeetcodeListAuthRow'),
        signIn: !!q('LeetcodeListSignIn'),
        rowCount: rows.length,
        sectionCount: sections.length,
        firstTitle: titles[0] ?? null,
        firstSection: sections[0] ?? null,
      };
    })()`);

    if (list?.hero !== 'Blind 75') {
      fail(`the problem list did not render its hero: ${JSON.stringify(list)}`);
      return;
    }
    // 72 problems across 11 categories. A row count of 0 under a rendered hero
    // is the shape a broken catalog import takes, and it reads as a styling bug.
    if (!list.rowCount || !list.sectionCount) {
      fail(`hero rendered but the catalog produced no rows: ${JSON.stringify(list)}`);
      return;
    }
    if (!list.authRow || !list.signIn) {
      fail(`the auth row is missing: ${JSON.stringify(list)}`);
      return;
    }

    // A plain CSS selector: the harness resolves it with querySelector (the
    // first of the 72 rows) and does not parse Playwright's `>> nth=0`.
    await container.tap({ name, ref: '[data-testid="LeetcodeListItem"]' });

    // Navigation plus an iframe load is not instant, and a fixed sleep here is
    // a coin flip.
    let editor = null;
    for (let attempt = 0; attempt < 16; attempt++) {
      await new Promise((r) => setTimeout(r, 500));
      editor = await evalInApp(`(() => {
        const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
        const zone = q('ProblemEditorEditorZone');
        const frame = zone ? zone.querySelector('iframe') : null;
        let editorReady = false;
        if (frame) {
          try {
            editorReady = !!frame.contentDocument && !!frame.contentDocument.querySelector('.cm-content');
          } catch (err) {
            // A cross-origin frame cannot be inspected. Say so rather than
            // reading it as "not ready yet" and timing out with no reason.
            editorReady = 'cross-origin: ' + err.message;
          }
        }
        return {
          screen: !!q('ProblemEditorScreen'),
          editorZone: !!zone,
          hasFrame: !!frame,
          editorReady,
          testInput: !!q('ProblemEditorTestInput'),
          actions: ['ProblemEditorFormatButton','ProblemEditorRunLocalButton','ProblemEditorRunButton','ProblemEditorResetButton'].filter((id) => !!q(id)),
          path: appDoc.location.pathname,
          starterCodeLength: (frame && frame.contentDocument && frame.contentDocument.querySelector('.cm-content'))
            ? frame.contentDocument.querySelector('.cm-content').textContent.length : 0,
        };
      })()`);
      if (editor?.editorReady === true && editor.starterCodeLength > 0) break;
    }

    if (!editor?.screen) {
      fail(`tapping a problem row did not open the editor screen: ${JSON.stringify(editor)}`);
      return;
    }
    if (editor.actions.length !== 4 || !editor.testInput) {
      fail(`the editor screen is missing controls: ${JSON.stringify(editor)}`);
      return;
    }
    if (editor.editorReady !== true) {
      fail(`the CodeMirror editor never came up inside the WebView: ${JSON.stringify(editor)}`);
      return;
    }
    // The starter code arrives over the same postMessage channel every edit
    // uses, so an empty document means that channel is dead even though the
    // iframe loaded.
    if (!editor.starterCodeLength) {
      fail(`CodeMirror loaded but holds no document, so the bridge is dead: ${JSON.stringify(editor)}`);
      return;
    }

    // Type into CodeMirror itself. The SDK routes keystrokes to the deepest
    // editable frame, which is exactly this contenteditable.
    const MARKER = 'verify_marker';
    await container.tap({ name, ref: '.cm-content' });
    await container.type({ name, ref: '.cm-content', text: MARKER });
    await new Promise((r) => setTimeout(r, 800));

    const typed = await evalInApp(`(() => {
      const frame = appDoc.querySelector('[data-testid="ProblemEditorEditorZone"] iframe');
      const cm = frame && frame.contentDocument && frame.contentDocument.querySelector('.cm-content');
      const text = cm ? cm.textContent : '';
      return { length: text.length, hasMarker: text.includes('${MARKER}'), head: text.slice(0, 40) };
    })()`);

    if (!typed?.hasMarker) {
      fail(`CodeMirror did not accept typing: ${JSON.stringify(typed)}`);
      return;
    }

    // The overlays are routes of their own (docs/overlays-as-routes.md), so
    // "the file exists" says nothing about whether the editor can reach one.
    await tapInApp('[data-testid="ProblemEditorInfoButton"]');
    let description = null;
    for (let attempt = 0; attempt < 10; attempt++) {
      await new Promise((r) => setTimeout(r, 400));
      description = await evalInApp(`(() => {
        const sheet = appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]');
        if (!sheet) return { open: false, path: appDoc.location.pathname };
        const r = sheet.getBoundingClientRect();
        const body = sheet.querySelector('[data-testid="ProblemDescriptionBody"]');
        return {
          open: r.top < (appDoc.defaultView.innerHeight - 40),
          path: appDoc.location.pathname,
          // Reads the sheet HEADER now: the standalone difficulty badge moved
          // up there next to the title. Left pointing at the old testID this
          // silently returned null and the check went vacuous.
          difficulty: (appDoc.querySelector('[data-testid="ProblemDescriptionSheetHeader"]') || {}).textContent || null,
          bodyHead: body ? body.textContent.slice(0, 40) : null,
        };
      })()`);
      if (description?.open && description.bodyHead) break;
    }
    if (!description?.open || !description.bodyHead) {
      fail(`the Info button did not open the description overlay: ${JSON.stringify(description)}`);
      return;
    }
    // Asserted, not merely logged. This value was printed into the ok-line and
    // nothing checked it, so when the badge moved into the sheet header the
    // line happily reported `shows "null"` and the run still passed.
    if (!description.difficulty || !/EASY|MEDIUM|HARD/.test(description.difficulty)) {
      fail(
        `the description sheet header lost its difficulty/category: ${JSON.stringify(description.difficulty)}`
      );
      return;
    }

    // The other half of "the overlay opened": that you can get back OUT of it.
    // Nothing in this suite asserted dismissal, and a sheet that rendered
    // perfectly and then swallowed every touch shipped twice because of it.
    await tapInApp('[data-testid="ProblemDescriptionSheetHeaderClose"]');
    let dismissed = null;
    for (let attempt = 0; attempt < 10; attempt++) {
      await new Promise((r) => setTimeout(r, 400));
      dismissed = await evalInApp(`(() => ({
        sheet: !!appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]'),
        editor: !!appDoc.querySelector('[data-testid="ProblemEditorScreen"]'),
        path: appDoc.location.pathname,
      }))()`);
      if (!dismissed?.sheet && dismissed?.editor) break;
    }
    if (dismissed?.sheet || !dismissed?.editor) {
      fail(`the description sheet's close button did not dismiss it: ${JSON.stringify(dismissed)}`);
      return;
    }

    const shot = await container.screenshot({ name });
    console.log(
      `ok: list "${list.hero}" — ${list.rowCount} rows in ${list.sectionCount} sections, ` +
        `first "${list.firstTitle}" under "${list.firstSection}"; editor at ${editor.path} with ` +
        `${editor.actions.length} actions; CodeMirror held ${editor.starterCodeLength} chars ` +
        `("${typed.head}…") and accepted "${MARKER}"; description overlay at ` +
        `${description.path} shows "${description.difficulty}" / "${description.bodyHead}…", ` +
        `and its close button returned to ${dismissed.path}`
    );
    if (shot?.path) console.log(`screenshot: ${shot.path}`);
  }
);
