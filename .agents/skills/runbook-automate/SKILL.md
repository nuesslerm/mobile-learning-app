---
paths: docs/runbooks/**, frontend/**
name: runbook-automate
description: >
  Use when the user asks to test a new Expo/React Native feature, create a runbook,
  write E2E tests, verify behavior on simulator or web, or says "test this",
  "validate the feature", "write tests for X", "create a runbook for X".
  Triggers on manual-first testing discipline before any automation is written.
metadata:
  category: testing
  claude_code: supported
  opencode: supported
  codex: manual-only
---

# Runbook Automate

Manual-first testing discipline. Reliable automated tests come from understanding the feature before automating it. Skipping straight to automation produces brittle, poorly-targeted coverage.

**The five-phase gate:**
```
Phase 1: EXPLORE  — manually drive the feature via MCP tools
Phase 2: RUNBOOK  — document every step for both web + Expo Go
Phase 3: VERIFY   — follow the runbook step-by-step on BOTH platforms
Phase 4: CHOOSE    — ask which automation target to use
Phase 5: AUTOMATE  — write the tests with the chosen tool
```

**Hard rules:**
- You cannot enter Phase 5 until Phase 3 passed on both platforms.
- You cannot enter Phase 5 until the user has explicitly chosen the automation target.
- If Phase 3 reveals a bug, fix it and restart Phase 3 before automating.

---

## Platform setup

Before any phase, confirm the environment:

```bash
wf test prepare --web | jq '{ok, reused, dir}'   # the web export of this tree, built or kept
```

Web target: a local `expo export --platform web` (`wf test prepare --web` builds one; NEVER the GitHub Pages URL).  
Expo Go target: the EAS Update CI published for the commit (a cold start opens it bare, `exp://u.expo.dev/<projectId>/group/<group>`: its manifest answers 404 under a `/--/` path while Expo Go is not running, `e2e/warm-app.lua`; a test's `base` is that link plus `/--`, which a route follows once the app runs; `against: update`; `wf test --help`), which CI's android-e2e-tests job loads; for a branch, `wf ship eas --initial-route <route>` publishes one.

---

## Phase 1: Explore

Explore on **both** platforms before writing anything down.

**Web (Playwright MCP):**
```
app_navigate_expo path=/<route>
app_snapshot
app_tap / app_type
app_screenshot
app_evaluate
```

**Expo Go (`wf android`, adb-backed):**
```
wf android take-screenshot <output.png>
wf android tap-on --id <testID>         # or --point x,y read off the screenshot; `wf android hierarchy` lists the ids
wf android swipe <x1>,<y1> <x2>,<y2>
wf android take-screenshot <output.png>
```

Exploration checklist:
- [ ] Happy path and every interactive element
- [ ] State changes after each action
- [ ] Persistence across reload or app restart
- [ ] Edge states: empty, error, disabled
- [ ] CSS variables for theme-driven features

Take notes as you go; finish exploring before writing the runbook so it reflects reality, not assumptions.

---

## Phase 2: Write the Runbook

Save to: `frontend/mla-tamagui/docs/runbooks/<feature-name>.md`

A runbook is a numbered, reproducible, platform-dual test script.

```markdown
# <Feature> — Manual Test Runbook

Verify behavior on BOTH web (Playwright MCP) and Expo Go (`wf android` / Maestro).
Complete all steps on one platform before switching.

---

## Web (Playwright MCP at 127.0.0.1:8800/mla-tamagui)

**Setup:**
\`\`\`bash
wf test prepare --web
\`\`\`

1. **<Step>** — <action>
   - Expected: <exact outcome>
   - Verify: `app_evaluate(...)` → <value>

---

## Expo Go (`wf android` / Maestro)

**Setup:**
\`\`\`bash
wf android open-link exp://u.expo.dev/<projectId>/group/<group>
# once the app runs, the route
wf android open-link exp://u.expo.dev/<projectId>/group/<group>/--/<route>
\`\`\`

1. **<Step>** — <action>
   - Expected: <exact outcome>
   - Verify: `wf android take-screenshot <output.png>` → <what to look for>

---

## Pass Criteria

All steps pass on BOTH platforms before automated tests are written.
```

Runbook rules:
- Every step is actionable and has an exact expected outcome.
- Each web step has a `wf android`/Maestro equivalent, and vice versa.
- Include a persistence/reload step for both platforms.
- No assumptions — if you don't know, add a step to find out.

---

## Phase 3: Verify the Runbook

Follow the runbook exactly on both platforms. For each step, record: ✅ pass / ❌ fail + actual behavior.

**Web:** execute each action with the Playwright MCP tools as written in the runbook.  
**Expo Go:** execute each action with the `wf android` verbs (`open-link`, `tap-on`, `swipe`, `input-text`, `take-screenshot`, the same Maestro-named verbs as `wf browser`) or Maestro as written in the runbook.

Failure handling:
1. Stop Phase 3.
2. Document the failure (step, platform, actual vs expected).
3. Fix the bug in the app code.
4. Restart Phase 3 from step 1 on both platforms.
5. Never proceed to Phase 4 with a known failure.

---

## Phase 4: Choose the Automation Target

After Phase 3 passes on both platforms, present the choice and wait for the user:

> Which automation target should we use?
> 1. **Maestro + iOS simulator** — native Expo Go flow, best for gestures, native UI, and app-store-style journeys.
> 2. **Playwright MCP + web Playwright tests** — web build, best for DOM assertions, CSS state, and fast CI feedback.

If the user wants both, run Phase 5 for one tool first, then the other.

---

## Phase 5: Automate

Only enter this phase after Phase 3 passed on both platforms AND the user has chosen a tool.

### Option A: Maestro

Write Maestro flows that drive the same steps as the runbook's Expo Go section. Tests live in:
`frontend/mla-tamagui/e2e/flows/<feature-name>.yml`

Each flow:
- Uses `arguments: { EXDevMenuIsOnboardingFinished: true, initialRoute: /<route> }` and `stopApp: true` for cold starts.
- Prefers `testID` selectors over text/location when available.
- Includes a final `assertVisible` or screenshot for the success criterion.

```yaml
appId: com.nuesslerm.expo-hello-world-app-2
---
- launchApp:
    stopApp: true
    arguments:
      EXDevMenuIsOnboardingFinished: true
      initialRoute: /leetcode/lectures
- assertVisible: "Two Sum"
- tapOn:
    testID: "LectureListItem-TwoSum"
- takeScreenshot: lecture-two-sum-solution
```

Run locally:

```bash
wf android open-link exp://u.expo.dev/<projectId>/group/<group>
wf android open-link exp://u.expo.dev/<projectId>/group/<group>/--/leetcode/lectures
maestro test frontend/mla-tamagui/e2e/flows/<feature-name>.yml
```

What to cover: happy path, native interactions, cold-start behavior.  
What NOT to cover: web-only CSS assertions, complex WebView text entry.

---

### Option B: Playwright MCP + web tests

Write automated tests that drive the same steps as the runbook's web section. Tests live in:
`frontend/mla-tamagui/e2e/web/<feature-name>.spec.ts`

Use the same MCP tools used in manual verification. **Never** write standalone `test-*.mjs` driver scripts — the MCP tools are the test surface.

```typescript
// frontend/mla-tamagui/e2e/web/<feature-name>.spec.ts
import { expect, test } from '@playwright/test';

test.describe('Lecture Two Sum — web', () => {
  test('lecture list renders and opens Two Sum', async ({ page }) => {
    await page.goto('http://127.0.0.1:8800/mla-tamagui/leetcode/lectures');
    await page.getByTestId('LectureListItem-TwoSum').click();
    await expect(page.getByTestId('LectureSolutionScreen-Title')).toBeVisible();
  });

  test('running the local test shows Accepted', async ({ page }) => {
    await page.goto('http://127.0.0.1:8800/mla-tamagui/leetcode/lectures/two-sum');
    await page.getByTestId('LectureSolutionScreen-RunTestButton').click();
    await expect(page.getByTestId('LectureSolutionScreen-ResultText')).toHaveText(/Accepted/);
  });
});
```

When writing raw Playwright `.spec.ts` files (not MCP-driven), use `@playwright/test` and the same selectors.

What to cover: happy path, state changes, persistence, DOM assertions.  
What NOT to cover: purely visual quality checks, real-device interactions, known platform failures.

---

## Checklist

```
Phase 1 — Explore
[ ] Navigated to feature on web and Expo Go
[ ] Exercised all interactive elements on both platforms
[ ] Captured stateful behaviors and CSS vars where relevant

Phase 2 — Runbook
[ ] Runbook saved to docs/runbooks/<feature>.md
[ ] Web steps: numbered, each with expected outcome + verify call
[ ] Expo Go steps: numbered, each with expected outcome + screenshot guidance
[ ] Persistence step included for both platforms

Phase 3 — Verify
[ ] All web steps: PASS ✅
[ ] All Expo Go steps: PASS ✅
[ ] No known failures outstanding

Phase 4 — Choose
[ ] User explicitly chose Maestro, Playwright, or both

Phase 5 — Automate
[ ] Test file at e2e/flows/<feature>.yml (Maestro) or e2e/web/<feature>.spec.ts (Playwright)
[ ] Each test name mirrors a runbook step
[ ] Assertions use DOM state / app_evaluate, not just screenshots
[ ] Tests cover happy path + persistence
[ ] Maestro cold-start uses EXDevMenuIsOnboardingFinished + initialRoute
```
