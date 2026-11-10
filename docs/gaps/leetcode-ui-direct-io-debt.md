---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/MermaidDiagram.tsx"
  - "frontend/mla-tamagui/features/leetcode/ui/ProblemListScreen.tsx"
  - "frontend/mla-tamagui/features/leetcode/ui/ProblemEditorScreen.tsx"
  - "frontend/mla-tamagui/features/leetcode/contracts/**"
  - "frontend/mla-tamagui/.dependency-cruiser.cjs"
opened: 2026-10-04
last_verified: 2026-10-21
verify: "! git grep -q \"from '../data/\" -- frontend/mla-tamagui/features/leetcode/ui"
links:
  - solutions/architecture/hexagonal-architecture-by-package-type.md
---
# Three leetcode UI files still import data/ or react-native-webview directly

## What is broken

`mobile-learn/no-direct-io-in-feature-ui` is `error` for `features/leetcode/ui/**`, but a file-scoped
override (`// debt: trunk-clean-arch`) downgrades it to `warn` for `MermaidDiagram.tsx`
(`react-native-webview`, line 5), `ProblemListScreen.tsx` (`../data/problemCatalog`, line 19) and
`ProblemEditorScreen.tsx` (`../data/problemCatalog` line 58, `../data/voiceEditIdentity` line 59).

## Why blocked

The ports have no diagram renderer or catalog loader yet.

## What unblocks it

Add a `MermaidDiagramPort` and a catalog-loading port to `contracts/ports.ts`, move the three files onto
them, and delete the override; `verify` exits 0.

## Re-checked 2026-10-20

The gap's verify still exits 1: the `// debt: trunk-clean-arch` override is at frontend/.oxlintrc.json:144-150. The three files still import I/O directly (MermaidDiagram.tsx:5, ProblemListScreen.tsx:11, ProblemEditorScreen.tsx:58-59), and contracts/ports.ts still has no diagram or catalog port. The oxlint migration (c5fdc265e) kept the override but turned the rule fully off rather than to warn, so oxlint over features/leetcode/ui exits 0 with no diagnostics.

Out of date above:

- "a file-scoped override (`// debt: trunk-clean-arch`) downgrades it to `warn`": the oxlint override now sets `mobile-learn/no-direct-io-in-feature-ui` to "off" for the three files (frontend/.oxlintrc.json:144-150). The lint script runs with --deny-warnings (frontend/mla-tamagui/package.json:12), so a warn would fail lint anyway. The rule is enforced by the oxlint jsPlugin mobile-learn (frontend/.oxlintrc.json:6), not ESLint.
- "`ProblemListScreen.tsx` (`../data/problemCatalog`, line 19)": the import is now at line 11.

## Re-checked 2026-10-20 (D4b)

The oxlint rule `no-direct-io-in-feature-ui` is gone (it enforced a layering convention, and the review found nothing it caught that the rest of the gates miss). The import half lives on in dependency-cruiser's `ui-never-imports-data` (`frontend/mla-tamagui/.dependency-cruiser.cjs`, run by `pnpm --filter mla-tamagui run arch`) with the same three files exempt, so the debt above is unchanged: those three files still import `../data/` or `react-native-webview`.

## Re-checked 2026-10-21

The old verify (`! grep -q 'debt: trunk-clean-arch' frontend/.oxlintrc.json`) exited 0 although the debt remained: the override it looked for went with the oxlint rule in D4b. The verify now greps the `ui/` imports of `../data/`, which exit 1 while ProblemListScreen.tsx:11 and ProblemEditorScreen.tsx:58-59 import `data/`.

Under the target in docs/solutions/architecture/hexagonal-architecture-by-package-type.md (type 8), a native view is ui, so MermaidDiagram.tsx's `react-native-webview` import is not debt and needs no diagram port. The data half is: `problemCatalog.ts` (a bundled catalogue) and `voiceEditIdentity.ts` (pure functions) move to `domain/`, and the two `pathNot` exemptions in `frontend/mla-tamagui/.dependency-cruiser.cjs` go with them.
