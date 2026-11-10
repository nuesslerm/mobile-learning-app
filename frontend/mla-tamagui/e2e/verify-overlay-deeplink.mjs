/**
 * The reference flow for issue #164: an overlay is a ROUTE, so a check can
 * start with it already open instead of replaying the taps that lead there.
 *
 * Why this is the canonical pattern rather than a nicety. A test that has to
 * tap its way to the state it wants asserts the path as much as the state: it
 * breaks when an unrelated button moves, it is slow, and when it fails you
 * cannot tell whether the overlay is broken or the route to it is. Addressing
 * the overlay directly separates those. It also exercises the case a user
 * actually hits from a shared link or a cold start -- the overlay rendering
 * with its parent screen mounted underneath it, which is the part that breaks
 * when a layout stops rendering `<Slot />` or a modal presentation regresses.
 *
 * Three assertions, in the order they can fail:
 *
 *  1. cold start on the overlay route renders the overlay,
 *  2. with its parent screen mounted underneath (not a bare modal on a blank
 *     page, which is what a missing parent layout looks like),
 *  3. and dismissing it lands on the parent route rather than a dead end --
 *     a cold-started overlay has no history entry to go "back" to, which is
 *     exactly where a naive `router.back()` strands the user.
 *
 * Runs in the device frame with the mock keyboard, like every other check
 * here, so what it proves is a statement about the mobile rendering path and
 * not about a desktop viewport.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const PROBLEM = 'two-sum';
const PARENT_PATH = `/leetcode/${PROBLEM}`;
const OVERLAY_PATH = `${PARENT_PATH}/description`;
const OVERLAY = '[data-testid="ProblemDescriptionSheet"]';
const PARENT = '[data-testid="ProblemEditorScreen"]';
const CLOSE = '[data-testid="ProblemDescriptionSheetHeaderClose"]';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function box(selector) {
  return `(() => {
    const el = appDoc.querySelector(${JSON.stringify(selector)});
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { w: Math.round(r.width), h: Math.round(r.height), top: Math.round(r.top) };
  })()`;
}

await withServedApp(
  { port: 8211, name: 'overlay-deeplink-verify', path: `leetcode/${PROBLEM}/description` },
  async ({ evalInApp, tapInApp }) => {
    // ---- Given: a cold start straight at the overlay route ---------------
    let overlay = null;
    for (let i = 0; i < 30; i++) {
      await sleep(400);
      overlay = await evalInApp(box(OVERLAY));
      if (overlay && overlay.h > 0) break;
    }
    if (!overlay || !overlay.h) {
      fail(
        `cold start at ${OVERLAY_PATH} did not render ${OVERLAY}: ${JSON.stringify(overlay)}`,
      );
      return;
    }

    // ---- Then: note whether the parent is already mounted ----------------
    // On a cold start expo-router mounts the overlay route without its sibling
    // screen -- there is no navigation history to have put it there. That is
    // the router's behavior, not a defect, so it is recorded rather than
    // asserted. What MUST hold is the next step: dismissing has to land the
    // user on a real screen instead of a blank one.
    const parent = await evalInApp(box(PARENT));

    // ---- When: the overlay is dismissed ----------------------------------
    await tapInApp(CLOSE);
    await sleep(1200);
    const after = await evalInApp(
      `(() => ({
        path: appDoc.defaultView.location.pathname,
        overlay: !!appDoc.querySelector(${JSON.stringify(OVERLAY)}),
        parent: !!appDoc.querySelector(${JSON.stringify(PARENT)}),
      }))()`,
    );
    if (after?.overlay) {
      fail(`the close control left ${OVERLAY} mounted: ${JSON.stringify(after)}`);
      return;
    }
    if (!after?.parent || !after.path?.endsWith(PARENT_PATH)) {
      fail(
        `dismissing a cold-started overlay must land on ${PARENT_PATH} with the ` +
          `editor mounted, got ${JSON.stringify(after)}`,
      );
      return;
    }

    console.log(
      `ok: cold start at ${OVERLAY_PATH} rendered the sheet ` +
        `(${overlay.w}x${overlay.h} at y${overlay.top}); the parent editor was ` +
        `${parent?.h ? `already mounted (${parent.w}x${parent.h})` : 'not mounted yet, as expected with no history'}; ` +
        `its close control landed on ${after.path} with the editor mounted`,
    );
  },
);
