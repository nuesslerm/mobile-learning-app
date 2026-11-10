---
status: open
area: mla-tamagui
paths:
  - "frontend/mla-tamagui/features/tamil-writing/routes/review.tsx"
  - "frontend/mla-tamagui/features/tamil-writing/ui/TamilReviewScreen.tsx"
opened: 2026-10-12
last_verified: 2026-10-22
links: []
---
# A cold load of /tamil/review?lessonId= from the web export hydrates with React #418

## What is broken

The export prerenders `/tamil/review` without a query, so the page holds the whole-course deck and the subtitle "Every letter". A cold load of `/tamil/review?lessonId=<id>` hydrates it with the lesson's own deck and "This lesson": React reports a text mismatch (#418) and `app.crash` fires, while the screen still renders after the client regenerates the tree.

Nothing in the app links to that URL (`TamilCourseScreen` pushes plain `/tamil/review`), so only a hand-typed or shared link meets it, and no e2e opens it. `tests/flow/coldDeepLinks.flow.test.tsx` pins it as an `it.fails` case with `app.render('/tamil/review', { prerender: 'export', search: { lessonId } })`.

## Why blocked

Not blocked.

## What unblocks it

Read `lessonId` after mount in `routes/review.tsx` and key the screen by it (the deck is built once, from the first `lessonId`), then turn the `it.fails` case into an `it` and close the gap through `/cf-compound`.

## Fixed 2026-10-22: the Tamil routes

`/tamil/<lesson>`, `/tamil/write/<subject>` and `/tamil/tracks/<track>` hydrate cleanly from the export; all 8 tamil-writing units and app-routes pass the zero-warnings gate on it.

Cause: the three routes had no static params, so the export held one template page per route and prerendered it with the literal `[lessonId]` (`[subjectId]`, `[trackId]`). The screens found no such lesson and printed `No lesson with id "[lessonId]"`; the browser, which knows the id from the URL, rendered the lesson, so the whole screen differed (the `TamilLessonMissing` tree against the `TamilLessonScreen` tree), not one text node.

Fix: each route exports `generateStaticParams` from the course data (`features/tamil-writing/data/pageParams.ts`, re-exported by the `app/` route file), so the export writes a page per lesson (24), track (6) and letter or word (268), `dist/tamil/uyir-1.html` and the same under the `(content)` and `(app)` groups. `wf test run`'s server already served a route's own `<id>.html` before the `[param]` page (`export_site.rs` needed no change; `tests/flow/pipeline.rs` now asserts it). An id with no page still gets the template page, so the three not-found messages no longer print the id and that page hydrates cleanly too.

Guards: `app.render(path, { prerender: 'export' })` in `@lib/flow-test-harness` server-renders the page the export holds for the path (the template page, with `[param]` literals and no query, unless `generateStaticParams` lists the params) and hydrates it at the real URL; `tests/flow/tamilColdLoad.flow.test.tsx` fails without the fix (8 of 8) and `e2e/app-routes.lua` opens the lesson, track, write and unknown-id routes cold on the export. Against an export built without the three `generateStaticParams`, app-routes and the first two tamil units failed with `app.crash` #418.

Cost: the export grows from 100 to 1292 pages (54 MB to 391 MB; `wf test prepare --web` 21 s to 72 s with a warm Metro cache; peak RSS 1.4 GB to 2.3 GB), about 1000 of them the write route's. CI's browser job already waits for 3000 MB of `MemAvailable` before it exports (`e2e-tests.yml`, "The VM has room for the export"), above the new peak, and samples the VM while it runs (`WF_VM_SAMPLE_SECS`). If the first CI export reads under about 1 GB free, the write route can prerender without the subject (show it after mount) and keep the template page only.

Fixed 2026-10-22, second pass: `/coding-agent/<threadId>` hydrated with #418 because `ThreadScreen` printed the id in its header and the template page holds `[threadId]` (thread ids are runtime data, so static params cannot list them). The header now shows the id after mount. `tests/flow/coldDeepLinks.flow.test.tsx` opens every dynamic route under `app/` cold with `prerender: 'export'` (the coding-agent thread case failed before the change) and fails when a new `[param]` route is not listed in it; `/leetcode/<id>/description` is the one route it cannot mount (jsdom cannot load its mermaid script), so `e2e/app-routes.lua` opens it on the export.

## History

### Re-checked 2026-10-20

A cold load of `/tamil/uyir-1` from an export at 982839ee9 raised #418 in two fresh `wf browser` sessions; `/tamil` and `/leetcode/two-sum` raised nothing. The route-restore claim of the first write-up was not observed.

### Re-checked 2026-10-21

A cold load of `/tamil/uyir-1` from a fresh export still raises #418.

A cold load of `/leetcode/two-sum` now raises nothing. That page's earlier #419 had a different cause: the static render loaded Tamagui twice, as CJS and as ESM, so the screen body found no TamaguiProvider and was left to the client. `metro.config.js` now resolves `tamagui` and `@tamagui/*` as ESM for the web, and `export:web` fails when an exported page holds a client-rendered boundary.

### Re-checked 2026-10-22: CI's browser e2e now runs on the export and tamil-writing fails on it

I ran every browser unit once through `wf test prepare --web` and `wf test run` at 0880c572f with local doubles for the backends. app-routes (1 unit), editor-typing (5) and voice-edit (8) passed. All 8 tamil-writing units passed every functional step (traced letters 97 to 99 % on the line, the write pad and words "LOOKS RIGHT") and failed only the zero-warnings gate, with `app.crash` "Minified React error #418" from the cold load of `/tamil/uyir-1`, `/tamil/uyirmei-k`, `/tamil/write/uyir-a` and `/tamil/write/word-uyir-*`; the hook sees it because the error is reported after hydration finishes.

The cause is in the prerendered page: `dist/(content)/tamil/[lessonId].html` says `No lesson with id "` and `"`, because the route has no static params and passes `lessonId ?? ''` while it is prerendered, and the client renders the lesson. The write route does the same with `subjectId`. A fix that fits the repo: render nothing until mounted in `features/tamil-writing/routes/[lessonId].tsx` and `routes/write/[subjectId].tsx` (as `lib/webview/ReactNativeWebView.web.tsx` does), then rerun the tamil-writing units. Until then the browser job's tamil-writing tests fail on every export; the gate is doing its job.

The export is a production build, so React's development warnings are compiled out and the zero-warnings gates of the four browser scripts see less than they did on the dev Metro; the #418 above still shows because React reports hydration errors in production too.

## Re-checked 2026-10-24

`tests/flow/coldDeepLinks.flow.test.tsx:55` still pins the case as `it.fails`, so the mismatch stands; the CI browser report of `app-routes` still carries React #418.
