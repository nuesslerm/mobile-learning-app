/**
 * Two overlays that landed on top of the thing they were describing.
 *
 * Both were found by exploration, not by a check, because a check only asserts
 * what its author already suspected -- and nobody suspects that the panel
 * reporting a failure is itself unreadable:
 *
 *   - the run-outcome card rendered at y438-501 under keyboard rails occupying
 *     y480-582, slicing off the line that carries the failure reason, at
 *     exactly the moment the user has typed and run;
 *   - the toast rendered at y16-90 over a header occupying y0-71, covering the
 *     problem title while telling you what just happened to it.
 *
 * REQUIRES A LOCAL BACKEND, started with the seed the app's development config
 * carries:
 *
 *   VM_GATEWAY_AUTH_SEED=local-e2e-token \
 *   LEETCODE_BACKEND_ADMIN_SECRET=local-e2e-admin \
 *   LEETCODE_CAPTURE_TOOL_BASE_URL=http://127.0.0.1:8792 \
 *   LEETCODE_CAPTURE_TRIGGER_SECRET=local-e2e-capture \
 *   pnpm --filter @app/leetcode-backend run dev
 *
 * That requirement is the point, not an inconvenience. The geometry assertions
 * pass just as happily on a 401 card, and for a long time that is all this path
 * produced -- `Run Local` and `Format` had NEVER executed for real in any
 * browser check, because the web export carried no backend token, so every
 * green run exercised only the error branch. The two "reached the backend
 * authenticated" assertions are what stop that reading as success again.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const problems = [];
function check(name, ok, detail) {
  problems.push({ name, ok, detail });
  console.log(`${ok ? '  ok  ' : '  FAIL'} ${name}${ok ? '' : ` — ${detail}`}`);
}

const SOLUTION = `class Solution:
    def twoSum(self, nums, target):
        seen = {}
        for i, n in enumerate(nums):
            if target - n in seen:
                return [seen[target - n], i]
            seen[n] = i
        return []`;

const RECT = (sel) => `(() => {
  const el = appDoc.querySelector(${JSON.stringify(sel)});
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { top: Math.round(r.top), bottom: Math.round(r.bottom) };
})()`;

await withServedApp(
  { port: 8243, name: 'result-toast', devicePreset: 'iphone-15-pro' },
  async ({ container, name, evalInApp, tapInApp, goto }) => {
    await goto('');
    await container.tap({
      name,
      ref: '[data-testid="LeetcodeListItem"]',
    });
    for (let i = 0; i < 25; i++) {
      await new Promise((r) => setTimeout(r, 400));
      if (
        await evalInApp(
          `!!appDoc.querySelector('[data-testid="ProblemEditorRunLocalButton"]')`
        )
      )
        break;
    }

    // Typing focuses the editor, which raises the rails. They have to be UP or
    // the clearance assertion below measures nothing and passes for free.
    await container.type({ name, ref: '.cm-content', text: SOLUTION });
    await new Promise((r) => setTimeout(r, 800));
    const rails = await evalInApp(RECT('[data-testid="EditorHelperRails"]'));
    check(
      'the keyboard rails are up before the clearance check',
      rails !== null,
      'the rails never mounted, so nothing could be covered and the next check would pass vacuously'
    );

    await tapInApp('[data-testid="ProblemEditorRunLocalButton"]');
    let card = null;
    let status = null;
    for (let i = 0; i < 40; i++) {
      await new Promise((r) => setTimeout(r, 500));
      card = await evalInApp(RECT('[data-testid="ProblemEditorRunOutcome"]'));
      status = await evalInApp(
        `(() => { const e = appDoc.querySelector('[data-testid="ProblemEditorRunOutcome"]'); return e ? e.textContent : null; })()`
      );
      if (card) break;
    }

    check(
      'the run outcome appeared',
      card !== null,
      'no outcome card rendered, so its position cannot be checked'
    );

    // The geometry checks pass just as happily on a 401 card, and for a long
    // time that is all this path ever produced: the export carried no backend
    // token, so `Run Local` and `Format` only ever exercised the error branch
    // while the suite reported green. Asserting the request was AUTHORISED is
    // what stops that reading as success again.
    check(
      'the run reached the backend authenticated',
      status !== null && !/unauthorized/i.test(status),
      `the outcome says ${JSON.stringify(status)} — the request was rejected, ` +
        `so nothing below tests a real run. Start the local backend with ` +
        `VM_GATEWAY_AUTH_SEED=local-e2e-token (see appConfig's development block).`
    );

    if (card && rails) {
      check(
        'the run outcome is not under the keyboard rails',
        card.bottom <= rails.top,
        `card ends at y${card.bottom}, rails start at y${rails.top} — ` +
          `${card.bottom - rails.top}px of the result is covered`
      );
    }

    // Toasts auto-dismiss, so a poll a second later finds nothing and reads as
    // "no toast fired". Arm the observer BEFORE the action that raises one.
    await evalInApp(`(() => {
      const w = appDoc.defaultView;
      w.__toastSeen = null;
      const header = appDoc.querySelector('[data-testid="ProblemEditorHeader"]');
      const hr = header ? header.getBoundingClientRect() : null;
      const obs = new w.MutationObserver(() => {
        if (w.__toastSeen) return;
        const t = [...appDoc.querySelectorAll('div')].find((d) => {
          const box = d.getBoundingClientRect();
          return /Formatted|Format failed|Run failed/.test(d.textContent || '')
            && box.height > 0 && box.height < 160;
        });
        if (!t) return;
        const r = t.getBoundingClientRect();
        w.__toastSeen = {
          toast: { top: Math.round(r.top), bottom: Math.round(r.bottom) },
          header: hr ? { top: Math.round(hr.top), bottom: Math.round(hr.bottom) } : null,
          text: (t.textContent || '').slice(0, 60),
        };
      });
      obs.observe(appDoc.body, { childList: true, subtree: true });
      return true;
    })()`);

    await tapInApp('[data-testid="ProblemEditorFormatButton"]');
    let toast = null;
    for (let i = 0; i < 30; i++) {
      await new Promise((r) => setTimeout(r, 300));
      toast = await evalInApp(`appDoc.defaultView.__toastSeen`);
      if (toast) break;
    }

    check(
      'the toast reports success, not an auth failure',
      toast !== null && !/failed/i.test(toast.text),
      `the toast says ${JSON.stringify(toast?.text)} — Format did not execute`
    );

    check(
      'a toast fired',
      toast !== null,
      'no toast was observed, so nothing could be covering the header and the next check would pass vacuously'
    );

    if (toast?.header) {
      check(
        'the toast clears the screen header',
        toast.toast.top >= toast.header.bottom,
        `toast starts at y${toast.toast.top}, header ends at y${toast.header.bottom} — ` +
          `it covers the title by ${toast.header.bottom - toast.toast.top}px`
      );
    }

    const shot = await container.screenshot({ name });
    if (shot?.path) console.log(`screenshot: ${shot.path}`);

    const failed = problems.filter((p) => !p.ok);
    if (failed.length) {
      fail(
        `${failed.length}/${problems.length} clearance checks failed: ` +
          failed.map((f) => f.name).join('; ')
      );
      return;
    }
    console.log(`ok: all ${problems.length} clearance checks passed`);
  }
);
