/**
 * The Tamil handwriting course renders, guides a stroke, and marks it.
 *
 * Three things here can bundle cleanly and still be dead, and none shows up in
 * an exit code: the course list is driven by an in-app catalogue (an import
 * that resolves to nothing reads as a styling bug, not a missing course), the
 * tracing pad is an `<svg>` under a gesture detector that can mount with no box
 * at all, and the stroke validator is only as good as the wrong strokes it has
 * refused -- a check that accepts everything passes every happy-path test.
 *
 * So the reference path is read back OUT of the page and then walked with a
 * real pointer, four ways: correctly, backwards, displaced, and cut short.
 *
 * Every app-level assertion goes through `evalInApp`, matching the other
 * checks in this directory.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const SURFACE = '[data-testid="TamilPracticeCanvasSurface"]';

await withServedApp(
  { port: 8226, name: 'tamil-verify', path: 'tamil' },
  async ({ container, name, evalInApp, tapInApp, goto }) => {
    const list = await evalInApp(`(() => {
      const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
      const rows = [...appDoc.querySelectorAll('[data-testid="TamilLessonRow"]')];
      const titles = [...appDoc.querySelectorAll('[data-testid="TamilLessonRowTitle"]')].map((e) => e.textContent);
      const sections = [...appDoc.querySelectorAll('[data-testid="TamilCourseSectionTitle"]')];
      const hero = q('TamilCourseHeroTitle');
      const heroBox = hero ? hero.getBoundingClientRect() : null;
      const banner = q('TamilCourseHero');
      const fill = q('TamilCourseOverallBarFill');
      const opaque = (v) => !!v && v !== 'rgba(0, 0, 0, 0)' && v !== 'transparent';
      return {
        hero: hero ? hero.textContent : null,
        heroPainted: !!heroBox && heroBox.width > 0,
        // The hero title is white, so an unresolved banner token is white on
        // white -- invisible, and indistinguishable from a render failure.
        heroBg: banner ? getComputedStyle(banner).backgroundColor : null,
        heroBgOpaque: banner ? opaque(getComputedStyle(banner).backgroundColor) : false,
        heroCount: q('TamilCourseHeroCountText') ? q('TamilCourseHeroCountText').textContent : null,
        overallBar: !!q('TamilCourseOverallBar'),
        // Nothing practised yet, so the fill must be zero-width. Tamagui drops
        // a percentage width of 0, which renders an empty bar as a full one.
        overallFillWidth: fill ? getComputedStyle(fill).width : null,
        rowCount: rows.length,
        sectionCount: sections.length,
        firstTitle: titles[0] ?? null,
      };
    })()`);

    if (!list?.hero || !list.heroPainted) {
      fail(`the course hero did not render: ${JSON.stringify(list)}`);
      return;
    }
    // Six base lessons plus one per consonant. A hero over zero rows is the
    // shape a broken catalogue import takes, and it reads as a styling bug.
    if (list.rowCount !== 24) {
      fail(`expected 24 lesson rows, got ${list.rowCount}: ${JSON.stringify(list)}`);
      return;
    }
    if (list.sectionCount !== 4) {
      fail(`expected 4 lesson sections, got ${list.sectionCount}: ${JSON.stringify(list)}`);
      return;
    }
    if (!list.overallBar) {
      fail(`the course progress bar is missing: ${JSON.stringify(list)}`);
      return;
    }
    if (!list.heroBgOpaque) {
      fail(`the hero banner has no background: ${JSON.stringify(list)}`);
      return;
    }
    if (list.overallFillWidth !== '0px') {
      fail(`an unpractised course reported progress: ${JSON.stringify(list)}`);
      return;
    }
    const listShot = await container.screenshot({ name });
    console.log(
      `ok: course list — ${list.rowCount} lessons in ${list.sectionCount} sections, hero "${list.hero}" on ${list.heroBg} (${listShot?.path ?? 'no shot'})`
    );

    await tapInApp('[data-testid="TamilLessonRow"]');

    let practice = null;
    for (let attempt = 0; attempt < 16; attempt++) {
      await new Promise((r) => setTimeout(r, 500));
      practice = await evalInApp(`(() => {
        const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
        const screen = q('TamilLessonScreen');
        if (!screen) return null;
        const outline = q('TamilPracticeCanvasOutline');
        const surface = q('TamilPracticeCanvasSurface');
        const svg = surface ? surface.querySelector('svg') : null;
        const outlineBox = outline ? outline.getBoundingClientRect() : null;
        const svgBox = svg ? svg.getBoundingClientRect() : null;
        const surfaceBox = surface ? surface.getBoundingClientRect() : null;
        // A canvas has no DOM paths, so the pad's fill falls back to the
        // surface's data-active-guide path (already in pad px) relative to the
        // surface box.
        const guideD = surface ? (surface.getAttribute('data-active-guide') || '') : '';
        const guideXs = guideD
          .split(/(?=[MQL])/)
          .map((piece) => piece.trim().split(' ').filter(Boolean).slice(1).map(Number))
          .filter((nums) => nums.length >= 2 && nums.every((n) => Number.isFinite(n)))
          .map((nums) => nums.slice(-2))
          .map(([x]) => x);
        const guideFill =
          guideXs.length > 0 && surfaceBox
            ? Math.round(
                ((Math.max(...guideXs) - Math.min(...guideXs)) / (surfaceBox.width || 1)) * 100
              )
            : 0;
        return {
          glyph: q('TamilPracticeGlyph') ? q('TamilPracticeGlyph').textContent : null,
          // The printed letter is the font's outline, filled -- the same
          // geometry the reference stroke came out of.
          surfacePainted: !!surfaceBox && surfaceBox.width > 0 && surfaceBox.height > 0,
          outlineFill: outlineBox ? Math.round((outlineBox.width / (svgBox ? svgBox.width : 1)) * 100) : guideFill,
          position: q('TamilPracticePosition') ? q('TamilPracticePosition').textContent : null,
          step: q('TamilPracticeStrokeStep') ? q('TamilPracticeStrokeStep').textContent : null,
          hint: q('TamilPracticeStrokeGuideHint') ? q('TamilPracticeStrokeGuideHint').textContent : null,
          letterHint: q('TamilPracticeStrokeHint') ? q('TamilPracticeStrokeHint').textContent : null,
          startDot: !!guideD,
          arrow: !!guideD,
          gotIt: !!q('TamilPracticeGotIt'),
        };
      })()`);
      if (practice && practice.surfacePainted && practice.startDot) break;
    }

    if (!practice) {
      fail('tapping a lesson row never reached the practice screen');
      return;
    }
    if (practice.glyph !== 'அ') {
      fail(`the practice screen showed the wrong letter: ${JSON.stringify(practice)}`);
      return;
    }
    // An outline that lays out at zero, or as a sliver, is an empty pad.
    if (!practice.surfacePainted || practice.outlineFill < 50) {
      fail(`the printed letter does not fill the pad: ${JSON.stringify(practice)}`);
      return;
    }
    if (!practice.surfacePainted) {
      fail(`the tracing surface has no box: ${JSON.stringify(practice)}`);
      return;
    }
    const strokeCount = Number(/Stroke 1 of (\d+)/.exec(practice.step ?? '')?.[1]);
    if (!strokeCount) {
      fail(`the stroke coach did not ask for the first stroke: ${JSON.stringify(practice)}`);
      return;
    }
    // The hints are teaching content, so they are checked, not just present.
    // Each is read off the stroke it describes, so it opens by saying where
    // that stroke starts -- and says something different from the letter's
    // own hint below it.
    if (!practice.hint?.startsWith('From the ')) {
      fail(`stroke 1's hint does not say what the hand does: ${JSON.stringify(practice)}`);
      return;
    }
    if (!practice.letterHint?.includes('Upper hook first')) {
      fail(`the letter's own stroke hint is missing: ${JSON.stringify(practice)}`);
      return;
    }
    if (!practice.startDot || !practice.arrow) {
      fail(`the pad shows no start dot or direction arrow: ${JSON.stringify(practice)}`);
      return;
    }
    const practiceShot = await container.screenshot({ name });
    console.log(
      `ok: practice — ${practice.position}, ${practice.step}, "${practice.hint}" (${practiceShot?.path ?? 'no shot'})`
    );

    /**
     * The reference path the app is CURRENTLY showing, as fractions of the pad.
     * Read out of the page rather than restated here, so the pointer walks the
     * same line the validator marks against -- a copy in this file could agree
     * with neither.
     */
    const activePath = () => evalInApp(`(() => {
      const surface = appDoc.querySelector('[data-testid="TamilPracticeCanvasSurface"]');
      if (!surface) return null;
      const d = surface.getAttribute('data-active-guide');
      if (!d) return null;
      const box = surface.getBoundingClientRect();
      // The reference path the app is showing, in pad px; each command's last
      // two numbers are the point the curve actually passes through.
      const points = d
        .split(/(?=[MQL])/)
        .map((piece) => piece.trim().split(' ').filter(Boolean).slice(1).map(Number))
        .filter((nums) => nums.length >= 2 && nums.every((n) => Number.isFinite(n)))
        .map((nums) => nums.slice(-2))
        .map(([x, y]) => ({ x: x / box.width, y: y / box.height }));
      return { points, width: box.width, height: box.height };
    })()`);

    const coach = () => evalInApp(`(() => {
      const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
      const surface = appDoc.querySelector('[data-testid="TamilPracticeCanvasSurface"]');
      return {
        step: q('TamilPracticeStrokeStep') ? q('TamilPracticeStrokeStep').textContent : null,
        hint: q('TamilPracticeStrokeGuideHint') ? q('TamilPracticeStrokeGuideHint').textContent : null,
        feedback: q('TamilPracticeFeedback') ? q('TamilPracticeFeedback').textContent : null,
        score: q('TamilPracticeStrokeScore') ? q('TamilPracticeStrokeScore').textContent : null,
        inkCount: surface ? Number(surface.getAttribute('data-ink-count') ?? 0) : 0,
      };
    })()`);

    const stroke1 = await activePath();
    if (!stroke1 || stroke1.points.length < 3) {
      fail(`could not read the reference path off the pad: ${JSON.stringify(stroke1)}`);
      return;
    }

    /** Walks the reference path with a real pointer, optionally distorted. */
    const write = async (points, transform) => {
      await container.stroke({
        name,
        ref: SURFACE,
        points,
        samplesPerSegment: 8,
        stepDelayMs: 8,
        ...(transform ? { transform } : {}),
      });
      await new Promise((r) => setTimeout(r, 350));
      return coach();
    };

    // 1..3. The wrong strokes, before any correct one. Each has to be refused
    // for its OWN reason -- one generic "try again" would pass all three and
    // teach nothing -- and must leave no ink behind.
    const wrong = [
      ['backwards', { reverse: true }, 'wrong way'],
      // Left and down, but still inside the pad: a path shifted off the
      // element never reaches the gesture at all, which would "pass" this
      // check while proving nothing.
      ['displaced', { offset: { x: -0.3, y: 0.1 } }, 'marked dot'],
      ['cut short', { truncate: 0.25 }, 'Too short'],
    ];
    for (const [label, transform, expected] of wrong) {
      const result = await write(stroke1.points, transform);
      if (result.step !== `Stroke 1 of ${strokeCount}`) {
        fail(`a ${label} stroke was accepted: ${JSON.stringify(result)}`);
        return;
      }
      if (!result.feedback?.includes(expected)) {
        // Also how a stroke that never landed shows up: the feedback is
        // simply the previous attempt's, unchanged.
        fail(`a ${label} stroke was refused with the wrong reason: ${JSON.stringify(result)}`);
        return;
      }
      if (result.inkCount !== 0) {
        fail(`a ${label} stroke left ink on the pad: ${JSON.stringify(result)}`);
        return;
      }
      console.log(`ok: ${label} stroke refused — "${result.feedback}"`);
    }
    const refusedShot = await container.screenshot({ name });
    console.log(`ok: pad still empty after three refusals (${refusedShot?.path ?? 'no shot'})`);

    // 4. Every stroke, correctly: the letter is finished and scored.
    let finished = await write(stroke1.points);
    if (finished.inkCount !== 1) {
      fail(`a correct first stroke was not accepted: ${JSON.stringify(finished)}`);
      return;
    }
    console.log(`ok: stroke 1 accepted — "${finished.feedback}"`);

    for (let stroke = 2; stroke <= strokeCount; stroke += 1) {
      const next = await activePath();
      if (!next || next.points.length < 2) {
        fail(`the pad did not advance to stroke ${stroke}: ${JSON.stringify(next)}`);
        return;
      }
      finished = await write(next.points);
    }
    if (!finished.step?.startsWith(`All ${strokeCount} stroke`)) {
      fail(`writing every stroke did not finish the letter: ${JSON.stringify(finished)}`);
      return;
    }
    if (!finished.score?.includes('% ON THE LINE')) {
      fail(`the finished letter was not scored: ${JSON.stringify(finished)}`);
      return;
    }
    const doneShot = await container.screenshot({ name });
    console.log(
      `ok: letter written — ${finished.step}, ${finished.score} (${doneShot?.path ?? 'no shot'})`
    );

    // The stroke coach pushed the buttons below the fold on a phone-sized
    // screen, and a tap at a point outside the viewport hit-tests as "nothing".
    await evalInApp(`(() => {
      const el = appDoc.querySelector('[data-testid="TamilPracticeGotIt"]');
      if (el) el.scrollIntoView({ block: 'center' });
      return true;
    })()`);
    await new Promise((r) => setTimeout(r, 400));
    await tapInApp('[data-testid="TamilPracticeGotIt"]');
    await new Promise((r) => setTimeout(r, 800));

    const graded = await evalInApp(`(() => {
      const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
      return {
        glyph: q('TamilPracticeGlyph') ? q('TamilPracticeGlyph').textContent : null,
        position: q('TamilPracticePosition') ? q('TamilPracticePosition').textContent : null,
        step: q('TamilPracticeStrokeStep') ? q('TamilPracticeStrokeStep').textContent : null,
      };
    })()`);

    if (!graded?.glyph || graded.glyph === 'அ') {
      fail(`grading a letter did not advance to another one: ${JSON.stringify(graded)}`);
      return;
    }
    if (graded.step !== 'Stroke 1 of 3' && !graded.step?.startsWith('Stroke 1')) {
      fail(`the next letter did not reset to its first stroke: ${JSON.stringify(graded)}`);
      return;
    }
    console.log(
      `ok: grading advanced to "${graded.glyph}" — ${graded.position}, ${graded.step}`
    );

    // Write it large: one screen, no scrolling. The header stays put, the
    // template is the font's own glyph (a vector, so crisp at any size), the
    // pad takes the rest of the screen and the controls sit at the bottom.
    await goto('tamil/write/uyir-a');
    await new Promise((r) => setTimeout(r, 900));

    const bigPad = () => evalInApp(`(() => {
      const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
      const surface = appDoc.querySelector('[data-testid="TamilWriteCanvasSurface"]');
      const box = surface ? surface.getBoundingClientRect() : null;
      const screen = q('TamilWriteScreen');
      const screenBox = screen ? screen.getBoundingClientRect() : null;
      const check = q('TamilWriteCheck');
      const checkBox = check ? check.getBoundingClientRect() : null;
      // A plain check, not a regex: a backslash class inside this template
      // literal collapses before it ever reaches the page.
      const steps = [...appDoc.querySelectorAll('[data-testid^="TamilWriteStep"]')]
        .filter((el) => {
          const id = el.getAttribute('data-testid') || '';
          const tail = id.slice('TamilWriteStep'.length);
          return tail.length > 0 && !Number.isNaN(Number(tail));
        })
        .map((el) => el.textContent);
      const glyph = q('TamilWriteCanvasGuideGlyph');
      const glyphBox = glyph ? glyph.getBoundingClientRect() : null;
      return {
        steps,
        padHeight: box ? Math.round(box.height) : 0,
        padWidth: box ? Math.round(box.width) : 0,
        // The template is the font glyph, not a traced outline: crisp, and
        // sized to the pad.
        vectorTemplate: !!glyph,
        rasterOutline: !!q('TamilWriteCanvasOutline'),
        templateHeight: glyphBox ? Math.round(glyphBox.height) : 0,
        // A word wider than the pad is clipped by it, and a person cannot copy what it hides.
        templateInside: !!glyphBox && !!box && glyphBox.left >= box.left && glyphBox.right <= box.right,
        templateWidth: glyphBox ? Math.round(glyphBox.width) : 0,
        // Everything on one screen: the check button's bottom edge is inside
        // the screen box, and nothing scrolls.
        checkBottom: checkBox ? Math.round(checkBox.bottom) : null,
        screenBottom: screenBox ? Math.round(screenBox.bottom) : null,
        overflow: screen ? screen.scrollHeight - screen.clientHeight : null,
        // expo-router's own bar shows the route file name when a screen is
        // not registered with headerShown:false.
        strayRouteHeader: appDoc.body.textContent.includes('tamil/write/['),
        headerTitle: q('TamilWriteHeaderTitle') ? q('TamilWriteHeaderTitle').textContent : null,
        hint: q('TamilWriteHint') ? q('TamilWriteHint').textContent : null,
        inkCount: surface ? Number(surface.getAttribute('data-ink-count') ?? 0) : 0,
      };
    })()`);

    const before = await bigPad();
    if (before.steps.length !== 2) {
      fail(`அ should be taught in two steps, got ${before.steps.length}: ${JSON.stringify(before)}`);
      return;
    }
    if (before.strayRouteHeader) {
      fail(`expo-router drew its own header above the screen's: ${JSON.stringify(before)}`);
      return;
    }
    if (!before.headerTitle?.includes('அ')) {
      fail(`the screen header does not name the letter: ${JSON.stringify(before)}`);
      return;
    }
    if (before.padHeight < 260) {
      fail(`the writing pad is only ${before.padHeight}px tall: ${JSON.stringify(before)}`);
      return;
    }
    if (!before.vectorTemplate || before.rasterOutline) {
      fail(`the template is not the crisp font glyph: ${JSON.stringify(before)}`);
      return;
    }
    if (before.templateHeight < before.padHeight * 0.5) {
      fail(`the template is small on the pad (${before.templateHeight}px of ${before.padHeight}px): ${JSON.stringify(before)}`);
      return;
    }
    if (before.overflow !== 0 || before.checkBottom > before.screenBottom) {
      fail(`the write screen does not fit on one screen: ${JSON.stringify(before)}`);
      return;
    }

    // Two strokes, wherever the hand likes -- no start dot, nothing refused.
    for (const points of [
      [
        { x: 0.15, y: 0.2 },
        { x: 0.5, y: 0.35 },
        { x: 0.35, y: 0.6 },
        { x: 0.7, y: 0.75 },
      ],
      [
        { x: 0.85, y: 0.15 },
        { x: 0.85, y: 0.8 },
      ],
    ]) {
      await container.stroke({
        name,
        ref: '[data-testid="TamilWriteCanvasSurface"]',
        points,
        samplesPerSegment: 8,
        stepDelayMs: 8,
      });
      await new Promise((r) => setTimeout(r, 300));
    }

    const written = await bigPad();
    if (written.inkCount !== 2) {
      fail(`the big pad kept ${written.inkCount} of 2 strokes: ${JSON.stringify(written)}`);
      return;
    }
    if (!written.hint?.includes('2 strokes')) {
      fail(`the pad did not count the strokes: ${JSON.stringify(written)}`);
      return;
    }
    const writeShot = await container.screenshot({ name });
    console.log(
      `ok: write large — ${before.steps.length} steps, ${before.padWidth}x${before.padHeight} pad, ` +
        `template ${before.templateHeight}px, fits on screen (overflow ${before.overflow}), ` +
        `2 strokes anywhere on it (${writeShot?.path ?? 'no shot'})`
    );

    // The ink is in the pad's pixels and the template is sized to the pad, so a
    // verdict that shrinks the pad leaves the writing off its template (G2).
    // Any verdict will do, an unreachable marker's included.
    await container.tap({ name, ref: '[data-testid="TamilWriteCheck"]' });
    let verdict = null;
    for (let i = 0; i < 60 && !verdict; i += 1) {
      await new Promise((r) => setTimeout(r, 500));
      verdict = await evalInApp(`(() => {
        const badge = appDoc.querySelector('[data-testid="TamilWriteVerdictBadge"]');
        return badge ? badge.textContent : null;
      })()`);
    }
    if (!verdict) {
      fail('Check showed no verdict');
      return;
    }
    const checked = await bigPad();
    if (checked.padHeight !== written.padHeight) {
      fail(
        `the verdict resized the pad from ${written.padHeight}px to ${checked.padHeight}px, ` +
          `so the ink no longer lies on the template: ${JSON.stringify(checked)}`
      );
      return;
    }
    console.log(`ok: the pad keeps its ${checked.padHeight}px when the verdict (${verdict}) shows`);

    // Tracks: the list, a track's tiles, and a word on the pad.
    await goto('tamil/tracks');
    await new Promise((r) => setTimeout(r, 900));
    const tracks = await evalInApp(`(() => {
      const rows = [...appDoc.querySelectorAll('[data-testid="TamilTrackRowTitle"]')].map((e) => e.textContent);
      const sections = [...appDoc.querySelectorAll('[data-testid="TamilTracksSectionTitle"]')].map((e) => e.textContent);
      return { rows, sections, stray: appDoc.body.textContent.includes('tamil/tracks') };
    })()`);
    if (tracks.sections.length !== 2 || tracks.rows.length < 5 || tracks.stray) {
      fail(`the track list is not right: ${JSON.stringify(tracks)}`);
      return;
    }

    await goto('tamil/tracks/first-words');
    await new Promise((r) => setTimeout(r, 900));
    const tiles = await evalInApp(`(() => {
      const glyphs = [...appDoc.querySelectorAll('[data-testid="TamilTrackTileGlyph"]')].map((e) => e.textContent);
      return { glyphs, header: (appDoc.querySelector('[data-testid="TamilTrackHeaderTitle"]') || {}).textContent || null };
    })()`);
    if (!tiles.glyphs.includes('அம்மா')) {
      fail(`the first-words track has no அம்மா tile: ${JSON.stringify(tiles)}`);
      return;
    }

    await goto('tamil/write/word-uyir-a');
    await new Promise((r) => setTimeout(r, 900));
    const word = await bigPad();
    if (word.steps.length !== 3 || !word.headerTitle?.includes('அம்மா') || word.overflow !== 0) {
      fail(`the word pad is not right: ${JSON.stringify(word)}`);
      return;
    }
    if (!word.templateInside) {
      fail(`the word's template runs past the pad (${word.templateWidth}px on a ${word.padWidth}px pad): ${JSON.stringify(word)}`);
      return;
    }
    const wordShot = await container.screenshot({ name });
    console.log(
      `ok: tracks — ${tracks.sections.length} sections, ${tracks.rows.length} tracks; ` +
        `${tiles.glyphs.length} tiles in first words; அம்மா on the pad in ${word.steps.length} steps (${wordShot?.path ?? 'no shot'})`
    );

    // Every letter, written through the pad with a real pointer. A guide that
    // does not lie on the letter it teaches is the failure that matters here,
    // and the only way to see it is to walk the guide and be marked against it.
    const walkLesson = async (lessonId, expected, stopAfterGuided = Infinity) => {
      await goto(`tamil/${lessonId}`);
      await new Promise((r) => setTimeout(r, 900));

      const scored = [];
      const freehand = [];
      for (let index = 0; index < expected; index += 1) {
        const current = await letter();
        if (!current?.glyph) {
          fail(`${lessonId} stopped showing letters at ${index}: ${JSON.stringify(current)}`);
          return null;
        }
        if (!current.guided) {
          freehand.push(current.glyph);
          await advance();
          continue;
        }
        if (scored.length >= stopAfterGuided) break;
        const total = Number(/Stroke 1 of (\d+)/.exec(current.step ?? '')?.[1]);
        if (!total) {
          fail(`"${current.glyph}" shows a guide but no stroke coach: ${JSON.stringify(current)}`);
          return null;
        }
        let result = null;
        for (let stroke = 1; stroke <= total; stroke += 1) {
          const path = await activePath();
          if (!path || path.points.length < 2) {
            fail(`"${current.glyph}" gave no path for stroke ${stroke}: ${JSON.stringify(path)}`);
            return null;
          }
          result = await write(path.points);
          if (result.inkCount !== stroke) {
            fail(
              `"${current.glyph}" refused its own stroke ${stroke} of ${total}: ${JSON.stringify(result)}`
            );
            return null;
          }
        }
        if (!result?.score?.includes('% ON THE LINE')) {
          fail(`"${current.glyph}" was not scored after ${total} strokes: ${JSON.stringify(result)}`);
          return null;
        }
        const percent = Number(/(\d+)% ON THE LINE/.exec(result.score)?.[1]);
        scored.push({ glyph: current.glyph, strokes: total, percent });
        console.log(`ok: ${current.glyph} — ${total} stroke(s) written, ${percent}% on the line`);
        await advance();
        if (scored.length >= stopAfterGuided) break;
      }
      return { scored, freehand };
    };

    const letter = () => evalInApp(`(() => {
      const q = (id) => appDoc.querySelector('[data-testid="' + id + '"]');
      const surface = appDoc.querySelector('[data-testid="TamilPracticeCanvasSurface"]');
      return {
        glyph: q('TamilPracticeGlyph') ? q('TamilPracticeGlyph').textContent : null,
        position: q('TamilPracticePosition') ? q('TamilPracticePosition').textContent : null,
        step: q('TamilPracticeStrokeStep') ? q('TamilPracticeStrokeStep').textContent : null,
        guided: !!(surface && surface.getAttribute('data-active-guide')),
      };
    })()`);

    const advance = async () => {
      await evalInApp(`(() => {
        const el = appDoc.querySelector('[data-testid="TamilPracticeGotIt"]');
        if (el) el.scrollIntoView({ block: 'center' });
        return true;
      })()`);
      await new Promise((r) => setTimeout(r, 300));
      await tapInApp('[data-testid="TamilPracticeGotIt"]');
      await new Promise((r) => setTimeout(r, 700));
    };

    // Every letter that stands on its own: the 12 உயிர், ஃ, and the 18
    // consonants -- which live at the head of their own உயிர்மெய் series,
    // since க is க்+அ and the bare மெய் form (க்) carries a புள்ளி instead of a
    // guide. Then the whole ம series, for a consonant carrying each vowel sign.
    const CONSONANTS = [
      'k', 'ng', 'c', 'nj', 'tt', 'nn', 't', 'n-dental', 'p',
      'y', 'r', 'l', 'v', 'zh', 'll', 'rr', 'n-alveolar',
    ];
    const BASE_LESSONS = [
      ['uyir-1', 6, Infinity],
      ['uyir-2', 6, Infinity],
      ['aaytham-grantha', 5, Infinity],
      ...CONSONANTS.map((key) => [`uyirmei-${key}`, 12, 1]),
      ['uyirmei-m', 12, Infinity],
    ];

    const everyLetter = [];
    const everyFreehand = [];
    for (const [lessonId, expected, limit] of BASE_LESSONS) {
      const walked = await walkLesson(lessonId, expected, limit);
      if (!walked) return;
      everyLetter.push(...walked.scored);
      everyFreehand.push(...walked.freehand);
    }

    // Writing a guide exactly and being told it is off the letter would mean
    // the guide and the printed letter disagree.
    const worst = Math.min(...everyLetter.map((entry) => entry.percent));
    if (everyLetter.length < 31) {
      fail(`only ${everyLetter.length} letters carried a stroke guide: ${JSON.stringify(everyLetter)}`);
      return;
    }
    if (worst < 80) {
      fail(`a guide scored ${worst}% against its own letter: ${JSON.stringify(everyLetter)}`);
      return;
    }

    const seriesShot = await container.screenshot({ name });
    console.log(
      `ok: every letter — ${everyLetter.length} guided letters written, ${worst}% worst on the line; ` +
        `${everyFreehand.length} freehand (${everyFreehand.join(' ')}) (${seriesShot?.path ?? 'no shot'})`
    );
  }
);
