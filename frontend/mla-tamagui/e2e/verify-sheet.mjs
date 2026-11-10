/**
 * Overlay behaviour, asserted rather than screenshotted.
 *
 * Written BEFORE the gorhom port, deliberately: the failure modes are
 * enumerable (see docs — the sheet field manual), so the checks can exist
 * first and prove they catch something. Each one names the specific defect it
 * guards, because a check whose failure message doesn't say what broke costs
 * more than it saves.
 *
 * What this file CANNOT see, and why the device pass still matters: there is
 * no native view controller in a browser, so the "portalled sheet under a
 * transparentModal route swallows every touch" bug is invisible here. It is
 * designed out structurally by using gorhom's non-portalling inline
 * `BottomSheet`, not detected.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const problems = [];
function check(name, ok, detail) {
  problems.push({ name, ok, detail });
  console.log(`${ok ? '  ok  ' : '  FAIL'} ${name}${ok ? '' : ` — ${detail}`}`);
}

await withServedApp(
  { port: 8217, name: 'sheet-verify', devicePreset: 'iphone-15-pro' },
  async ({ container, name, evalInApp, tapInApp, goto }) => {
    await container.tap({
      name,
      ref: '[data-testid="LeetcodeListItem"]',
    });
    for (let i = 0; i < 16; i++) {
      await new Promise((r) => setTimeout(r, 400));
      if (
        await evalInApp(
          `!!appDoc.querySelector('[data-testid="ProblemEditorScreen"]')`
        )
      )
        break;
    }

    // ---------------------------------------------------------------- menu
    // The reported bug: the header menu was seen rendering off-screen. That is
    // a collision-detection failure, and it is what floating-ui's shift()
    // middleware exists to fix. Assert the panel is fully inside the viewport.
    await tapInApp('[data-testid="ProblemEditorMenuButton"]');
    let menu = null;
    for (let i = 0; i < 10; i++) {
      await new Promise((r) => setTimeout(r, 300));
      menu = await evalInApp(`(() => {
        const el = appDoc.querySelector('[data-testid="ProblemEditorMenuButtonContent"]');
        if (!el) return null;
        const r = el.getBoundingClientRect();
        const w = appDoc.defaultView;
        return {
          left: Math.round(r.left), right: Math.round(r.right),
          top: Math.round(r.top), bottom: Math.round(r.bottom),
          vw: w.innerWidth, vh: w.innerHeight,
        };
      })()`);
      if (menu) break;
    }
    if (!menu) {
      check('menu opens', false, 'the menu panel never rendered');
    } else {
      const inside =
        menu.left >= 0 &&
        menu.top >= 0 &&
        menu.right <= menu.vw &&
        menu.bottom <= menu.vh;
      check(
        'menu stays inside the viewport',
        inside,
        `panel at [${menu.left},${menu.top}]-[${menu.right},${menu.bottom}] in ${menu.vw}x${menu.vh}`
      );
    }
    await tapInApp('[data-testid="ProblemEditorMenuButton"]');
    await new Promise((r) => setTimeout(r, 500));

    // -------------------------------------------------------------- backdrop
    // gorhom's backdrop defaults to `appearsOnIndex: 1`. At a single snap point
    // you sit at index 0, so the backdrop never renders and there is nothing to
    // tap — half of the "sheet cannot be dismissed" bug, and the half a browser
    // can see. This check is what stops the port reintroducing it.
    await tapInApp('[data-testid="ProblemEditorInfoButton"]');
    let sheet = null;
    for (let i = 0; i < 12; i++) {
      await new Promise((r) => setTimeout(r, 400));
      sheet = await evalInApp(`(() => {
        const el = appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]');
        if (!el) return null;
        const r = el.getBoundingClientRect();
        const overlay = appDoc.querySelector('[aria-label="Bottom sheet backdrop"]');
        const or = overlay ? overlay.getBoundingClientRect() : null;
        // When the handle is missing, say what IS on screen — a failure message
        // that only reports absence sends you hunting for the wrong thing.
        const siblings = [...appDoc.querySelectorAll('div')]
          .filter((n) => {
            const b = n.getBoundingClientRect();
            return b.width > 200 && b.height > 200 && n.children.length < 3;
          })
          .slice(0, 6)
          .map((n) => n.getAttribute('data-testid') || ('div.' + String(n.className).slice(0, 40)));
        return {
          open: r.top < appDoc.defaultView.innerHeight - 40,
          top: Math.round(r.top),
          hasOverlay: !!overlay,
          overlayArea: or ? Math.round(or.width * or.height) : 0,
          candidates: siblings,
        };
      })()`);
      if (sheet?.open) break;
    }
    if (!sheet?.open) {
      fail(`the description sheet never opened: ${JSON.stringify(sheet)}`);
      return;
    }
    check(
      'backdrop is rendered while the sheet is open',
      sheet.hasOverlay && sheet.overlayArea > 0,
      `hasOverlay=${sheet.hasOverlay} area=${sheet.overlayArea}; large elements present: ${JSON.stringify(sheet.candidates)}`
    );

    // ------------------------------------------------------------ body scroll
    // A gorhom scrollable without `flex: 1` sizes to its content and silently
    // refuses to scroll; a virtualized list nested inside one does the same.
    const scrolled = await evalInApp(`(() => {
      const sv = appDoc.querySelector('[data-testid="ProblemDescriptionScroll"]');
      if (!sv) return { ok: false, reason: 'no scroll surface' };
      const before = sv.scrollTop;
      sv.scrollTop = 250;
      return { ok: sv.scrollTop > before, before, after: sv.scrollTop, height: sv.scrollHeight };
    })()`);
    check(
      'sheet body actually scrolls',
      scrolled?.ok === true,
      `scrollTop ${scrolled?.before} -> ${scrolled?.after} (content ${scrolled?.height}px)`
    );

    // -------------------------------------------------- backdrop tap dismisses
    const overlayTap = await evalInApp(`(() => {
      const o = appDoc.querySelector('[aria-label="Bottom sheet backdrop"]');
      if (!o) return { ok: false, reason: 'no overlay to tap' };
      const r = o.getBoundingClientRect();
      // Near the top edge: inside the backdrop, clear of the sheet frame.
      const x = Math.round(r.left + r.width / 2);
      const y = Math.round(r.top + 30);
      const hit = appDoc.elementFromPoint(x, y);
      if (!hit) return { ok: false, reason: 'nothing at the backdrop point' };
      // The full sequence react-native-web's responder expects — a bare
      // pointerdown/up/click is silently ignored, which reads as "the backdrop
      // does not dismiss" when in fact it was never pressed.
      const opts = { bubbles: true, cancelable: true, view: appDoc.defaultView, button: 0, clientX: x, clientY: y };
      hit.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerdown', { ...opts, pointerId: 1, isPrimary: true }));
      hit.dispatchEvent(new appDoc.defaultView.MouseEvent('mousedown', opts));
      hit.dispatchEvent(new appDoc.defaultView.PointerEvent('pointerup', { ...opts, pointerId: 1, isPrimary: true }));
      hit.dispatchEvent(new appDoc.defaultView.MouseEvent('mouseup', opts));
      hit.dispatchEvent(new appDoc.defaultView.MouseEvent('click', opts));
      return { ok: true, hit: hit.getAttribute('data-testid') || hit.tagName };
    })()`);
    let afterBackdrop = null;
    let frameTop = null;
    for (let i = 0; i < 10; i++) {
      await new Promise((r) => setTimeout(r, 350));
      const state = await evalInApp(`(() => {
        const el = appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]');
        if (!el) return { present: false, top: null };
        return { present: true, top: Math.round(el.getBoundingClientRect().top) };
      })()`);
      afterBackdrop = state.present;
      frameTop = state.top;
      if (!afterBackdrop) break;
    }
    check(
      'tapping the backdrop dismisses the sheet',
      afterBackdrop === false,
      // The frame position distinguishes "the press never landed" (top
      // unchanged) from "gorhom closed but the route did not pop" (top moved
      // off-screen) — two different bugs with the same symptom.
      `sheet still present after tapping ${overlayTap?.hit ?? 'nothing'}; frame top now ${frameTop} (was ${sheet.top})`
    );

    // --------------------------------------------------- reopen after dismiss
    // The controlled-open race: a `useEffect -> ref.open()` bridge races
    // gorhom's own `index` prop and silently fails the FIRST re-open. Only a
    // second open/close cycle catches it.
    await goto('');
    for (let i = 0; i < 12; i++) {
      await new Promise((r) => setTimeout(r, 300));
      if (
        await evalInApp(
          `!!appDoc.querySelector('[data-testid="LeetcodeListItem"]')`
        )
      )
        break;
    }
    await container.tap({
      name,
      ref: '[data-testid="LeetcodeListItem"]',
    });
    for (let i = 0; i < 16; i++) {
      await new Promise((r) => setTimeout(r, 400));
      if (
        await evalInApp(
          `!!appDoc.querySelector('[data-testid="ProblemEditorScreen"]')`
        )
      )
        break;
    }
    await tapInApp('[data-testid="ProblemEditorInfoButton"]');
    let reopened = false;
    for (let i = 0; i < 12; i++) {
      await new Promise((r) => setTimeout(r, 400));
      reopened = await evalInApp(`(() => {
        const el = appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]');
        return !!el && el.getBoundingClientRect().top < appDoc.defaultView.innerHeight - 40;
      })()`);
      if (reopened) break;
    }
    check(
      'sheet reopens after a dismiss',
      reopened === true,
      'second open never rendered'
    );

    // --------------------------------------------------------- explicit close
    await tapInApp('[data-testid="ProblemDescriptionSheetHeaderClose"]');
    let closed = false;
    for (let i = 0; i < 10; i++) {
      await new Promise((r) => setTimeout(r, 350));
      closed = !(await evalInApp(
        `!!appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]')`
      ));
      if (closed) break;
    }
    check('close button dismisses the sheet', closed, 'the ✕ did not close it');

    // ------------------------------------------------ unsaved-changes guard
    // The editor buffer is useState seeded from starter code and persisted
    // nowhere, so leaving really does lose it. Dirty the buffer, press back,
    // and the guard must interrupt rather than navigating away silently.
    await container.tap({ name, ref: '.cm-content' });
    await container.type({ name, ref: '.cm-content', text: 'dirty_marker' });
    await new Promise((r) => setTimeout(r, 700));

    await tapInApp('[data-testid="ProblemEditorHeaderBack"]');
    let guard = null;
    for (let i = 0; i < 12; i++) {
      await new Promise((r) => setTimeout(r, 400));
      guard = await evalInApp(`(() => ({
        dialog: !!appDoc.querySelector('[data-testid="UnsavedChangesDialog"]'),
        stillEditor: !!appDoc.querySelector('[data-testid="ProblemEditorScreen"]'),
        path: appDoc.location.pathname,
      }))()`);
      if (guard?.dialog) break;
    }
    check(
      'leaving a dirty editor is interrupted',
      guard?.dialog === true,
      `no dialog; at ${guard?.path} (editor present: ${guard?.stillEditor})`
    );

    // Appearing is half a dialog. The other half is that its buttons can be
    // pressed -- the exact thing that failed twice on the sheet, where the
    // control rendered perfectly and something above it ate the touch.
    if (guard?.dialog) {
      await tapInApp('[data-testid="UnsavedChangesDialog-Cancel"]');
      let kept = null;
      for (let i = 0; i < 12; i++) {
        await new Promise((r) => setTimeout(r, 400));
        kept = await evalInApp(`(() => ({
          dialog: !!appDoc.querySelector('[data-testid="UnsavedChangesDialog"]'),
          editor: !!appDoc.querySelector('[data-testid="ProblemEditorScreen"]'),
        }))()`);
        if (!kept.dialog) break;
      }
      check(
        '"Keep editing" dismisses and stays in the editor',
        kept?.dialog === false && kept?.editor === true,
        `dialog=${kept?.dialog} editor=${kept?.editor}`
      );
    }

    // The keyboard must go away when a sheet comes up. On a device it does not
    // just look wrong -- the soft keyboard is drawn ON TOP of the sheet and
    // covers the control the sheet exists to offer, which is exactly how the
    // language dial shipped. Raising the harness keyboard by focusing the
    // editor makes this assertable here: the daemon watches focus, so a real
    // blur lowers the mock keyboard the same way it lowers a real one.
    // Same route in as the top of this file: a fresh load plus a list tap. A
    // direct goto to the editor URL raced CodeMirror's iframe and left the tap
    // below with nothing to hit.
    await goto('');
    await container.tap({
      name,
      ref: '[data-testid="LeetcodeListItem"]',
    });
    let editorReady = false;
    for (let i = 0; i < 30; i++) {
      await new Promise((r) => setTimeout(r, 500));
      editorReady = await evalInApp(`(() => {
        const zone = appDoc.querySelector('[data-testid="ProblemEditorEditorZone"]');
        const frame = zone ? zone.querySelector('iframe') : null;
        return !!(frame && frame.contentDocument && frame.contentDocument.querySelector('.cm-content'));
      })()`);
      if (editorReady) break;
    }
    check(
      'the editor is up before the keyboard checks',
      editorReady === true,
      'CodeMirror never mounted, so there is no keyboard to raise'
    );

    const READING = `(() => {
      const view = appDoc.defaultView;
      const active = appDoc.activeElement;
      return {
        rail: !!appDoc.querySelector('[data-testid="EditorHelperRails"]'),
        harnessUp: view.__MCP_KEYBOARD_VISIBLE__ === true,
        editorFocused: !!active && active.tagName.toLowerCase() === 'iframe',
      };
    })()`;

    if (editorReady) await container.tap({ name, ref: '.cm-content' });

    let raised = null;
    for (let i = 0; editorReady && i < 12; i++) {
      await new Promise((r) => setTimeout(r, 400));
      raised = await evalInApp(READING);
      if (raised?.rail) break;
    }
    if (editorReady)
      check(
        'the keyboard is up before the sheet opens',
        raised?.rail === true,
        `rail=${raised?.rail} harnessUp=${raised?.harnessUp} — nothing to dismiss, so the next check would pass vacuously`
      );

    if (raised?.rail) {
      await tapInApp('[data-testid="ProblemEditorLanguagePicker-Trigger"]');
      let after = null;
      for (let i = 0; i < 12; i++) {
        await new Promise((r) => setTimeout(r, 400));
        after = await evalInApp(READING);
        if (!after?.rail && !after?.editorFocused) break;
      }
      const dialUp = await evalInApp(
        `!!appDoc.querySelector('[data-testid="ProblemEditorLanguagePicker-Python"]')`
      );
      check(
        'opening a sheet puts the keyboard away',
        after?.editorFocused === false && after?.rail === false,
        `editorFocused=${after?.editorFocused} rail=${after?.rail} — the keyboard stays over the sheet`
      );
      check(
        'the dial is on screen with the keyboard gone',
        dialUp === true,
        'the language rows never rendered'
      );
    }

    // A disabled control must LOOK disabled. `disabled` alone sets aria-disabled
    // and pointer-events:none, both invisible -- so without a disabledStyle the
    // button is pixel-identical to the enabled ones beside it, and the user taps
    // it repeatedly while nothing happens. Run is disabled here because nobody
    // is signed in, which makes the editor screen a free fixture for this.
    const buttons = await evalInApp(`(() => {
      const read = (id) => {
        const el = appDoc.querySelector('[data-testid="' + id + '"]');
        if (!el) return null;
        const cs = appDoc.defaultView.getComputedStyle(el);
        return {
          opacity: Number(cs.opacity),
          disabled: el.getAttribute('aria-disabled') === 'true',
        };
      };
      return { run: read('ProblemEditorRunButton'), runLocal: read('ProblemEditorRunLocalButton') };
    })()`);

    check(
      'the disabled Run button is actually marked disabled',
      buttons?.run?.disabled === true && buttons?.runLocal?.disabled === false,
      `run.disabled=${buttons?.run?.disabled} runLocal.disabled=${buttons?.runLocal?.disabled} — ` +
        `without this the next check would compare two enabled buttons and pass vacuously`
    );
    if (
      buttons?.run?.disabled === true &&
      buttons?.runLocal?.disabled === false
    ) {
      check(
        'a disabled button is visually distinguishable from an enabled one',
        buttons.run.opacity < buttons.runLocal.opacity,
        `both render at opacity ${buttons.run.opacity} — a disabled control that ` +
          `looks enabled is a control the user taps while nothing happens`
      );
    }

    // A bottom sheet's bottom edge is the screen's bottom edge. If it floats
    // short of it the sheet is anchored to a container that is not the screen,
    // which on a device reads as content that "doesn't reach the bottom" with
    // dead space underneath.
    // Reached by tapping the info button, exactly as a user does. A direct
    // goto to the overlay route does not mount the sheet -- it is a
    // transparentModal that expects to be pushed over the editor.
    await goto('');
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
    let frame = null;
    let previousBottom = null;
    // The sheet spring-animates into place, so the FIRST poll to see the
    // element can still catch it mid-flight (a bottom that hasn't reached its
    // resting value yet reads as "short of the screen" even when the fix is
    // correct). Only stop once two consecutive reads agree it has settled.
    for (let i = 0; i < 20; i++) {
      await new Promise((r) => setTimeout(r, 400));
      frame = await evalInApp(`(() => {
        const view = appDoc.defaultView;
        const el = appDoc.querySelector('[data-testid="ProblemDescriptionSheet"]');
        if (!el) return null;
        const r = el.getBoundingClientRect();
        // Walk up so a short ancestor is visible in the failure message: the
        // sheet is only ever as tall as whatever gorhom measured as its
        // container.
        const chain = [];
        let node = el;
        for (let i = 0; i < 10 && node; i++) {
          const nr = node.getBoundingClientRect();
          const cs = view.getComputedStyle(node);
          chain.push(
            (node.getAttribute('data-testid') || node.tagName) +
              ':' + Math.round(nr.top) + '-' + Math.round(nr.bottom) +
              ' [pos=' + cs.position + ' h=' + cs.height + ' minH=' + cs.minHeight +
              ' flex=' + cs.flex + ' display=' + cs.display +
              ' top=' + cs.top + ' bottom=' + cs.bottom +
              ' cls=' + String(node.className).slice(0, 60) + ']'
          );
          node = node.parentElement;
        }
        return {
          top: Math.round(r.top),
          bottom: Math.round(r.bottom),
          viewport: view.innerHeight,
          chain: chain.join(' < '),
          harnessKeyboard: view.__MCP_KEYBOARD_VISIBLE__ === true,
          keyboardHeight: view.__MCP_KEYBOARD_HEIGHT__ ?? 0,
          railMounted: !!appDoc.querySelector('[data-testid="EditorHelperRails"]'),
        };
      })()`);
      if (frame && frame.bottom === previousBottom) break;
      previousBottom = frame ? frame.bottom : null;
    }
    check(
      'the description sheet mounted',
      frame !== null,
      'ProblemDescriptionSheet never rendered, so its geometry cannot be measured'
    );
    if (frame) {
      check(
        'the sheet is anchored to the bottom of the screen',
        Math.abs(frame.viewport - frame.bottom) <= 2,
        `sheet bottom ${frame.bottom} vs viewport ${frame.viewport} — ` +
          `${frame.viewport - frame.bottom}px of dead space below it. ` +
          `harnessKeyboard=${frame.harnessKeyboard} kbHeight=${frame.keyboardHeight} ` +
          `rail=${frame.railMounted} top=${frame.top} | ${frame.chain.split(' < ').join('\n    < ')}`
      );
    }

    const failed = problems.filter((p) => !p.ok);
    const shot = await container.screenshot({ name });
    if (shot?.path) console.log(`screenshot: ${shot.path}`);
    if (failed.length) {
      fail(
        `${failed.length}/${problems.length} overlay checks failed: ` +
          failed.map((f) => f.name).join('; ')
      );
      return;
    }
    console.log(`ok: all ${problems.length} overlay checks passed`);
  }
);
