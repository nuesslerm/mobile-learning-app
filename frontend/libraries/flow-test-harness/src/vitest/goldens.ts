// Optional extension, added to setupFiles only when FLOW_GOLDENS_OUT is set (`wf flow run`): every test's final
// screen is written as a golden snapshot. A plain `vitest run` never loads this file, so it never pays for it.
import { createHash } from 'node:crypto';

import { act } from '@testing-library/react';
import { afterEach, vi } from 'vitest';

import { layoutBoxes } from '../interactions/layout';
import { snapshotPage } from '../interactions/snapshot';

// Every flow test's final screen is a golden; the hash keeps two long names with one prefix apart.
function finalStep(testName: string): string {
  const hash = createHash('sha1').update(testName).digest('hex').slice(0, 6);
  return `final-${testName.slice(0, 48)}-${hash}`;
}

const QUIET_MS = 300;
const EDITOR_QUIET_MS = 1000;
const SETTLE_CAP_MS = 3000;

// A test may end with renders still queued (CodeMirror highlights in idle time); the golden waits for a quiet DOM.
async function settle(): Promise<void> {
  if (vi.isFakeTimers()) return;
  const frames = Array.from(
    document.querySelectorAll('iframe'),
    (f) => f.contentDocument
  );
  const docs = [document, ...frames.filter((d): d is Document => d !== null)];
  let last = Date.now();
  const observers = docs.map((d) => {
    const o = new MutationObserver(() => {
      last = Date.now();
    });
    o.observe(d, {
      subtree: true,
      childList: true,
      attributes: true,
      characterData: true,
    });
    return o;
  });
  // a language switch loads its parser asynchronously, with no DOM change until the colours arrive
  const quiet = docs.some((d) => d.querySelector('.cm-editor'))
    ? EDITOR_QUIET_MS
    : QUIET_MS;
  const started = Date.now();
  await act(async () => {
    while (Date.now() - last < quiet && Date.now() - started < SETTLE_CAP_MS) {
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  });
  observers.forEach((o) => o.disconnect());
}


// registered after setup.ts, so it unwinds before that file's cleanup
afterEach(async (ctx) => {
  if (document.body.childElementCount > 0) {
    await settle();
    snapshotPage(finalStep(ctx.task.name), { layouts: layoutBoxes() });
  }
});
