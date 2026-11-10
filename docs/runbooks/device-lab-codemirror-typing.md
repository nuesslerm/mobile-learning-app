---
title: device-lab G3 — code typed into the CodeMirror editor like a person
verifies: On the device-lab render (the lab web export at iPhone 15 Pro size, live backends), an agent writes two leetcode solutions into the CodeMirror editor key by key through the phone keyboard and the syntax toolbar — caret placed by a tap, Return with auto-indent, auto-closed brackets typed over, a typo fixed with Backspace — and Run Local passes every test
tier: agent-driven
packages: frontend/mla-tamagui, frontend/libraries/tamagui-design-system, clis/wf-harness
maxAgeDays: 30
---

# device-lab G3: code typed into CodeMirror like a person

Gate G3 of the device-lab spec ([`spec.md`](../plans/device-lab/spec.md) §7). The editor is CodeMirror 6 in a
WebView on the phone and in a srcdoc iframe in the browser. On a phone a person taps into it, the keyboard
comes up, and every character goes in at the caret through the input method. This gate makes the agent
write code that way, and nothing else.

An agent runs it step by step through `wf browser`, on a commit that is already on `origin/dev`, and writes the run to `docs/runbooks/.results/device-lab-codemirror-typing.json` (`runbook`, `sha`, `ranAt`, `verdict`, `evidence`).
It needs the phase 3 driver work: touch taps (I13), the soft-keyboard model with
`wf browser input-text` and `wf browser press-key` (I14), the input log and `wf browser check-taps`.

## Rules: the human input model

- **Act only through input.** Taps go through `wf browser tap-on`; text through
  `wf browser input-text`, which types key by key at the caret at a thumb's pace and never clears the buffer;
  single keys through `wf browser press-key` (Enter, Backspace, and the arrows by Maestro's names: `"Remote Dpad Left"`,
  `"Remote Dpad Right"`, `"Remote Dpad Up"`, `"Remote Dpad Down"`). `wf browser get-element` and `wf browser hierarchy`
  read the page; nothing clicks, focuses, sets a value or dispatches an event.
- **No shortcuts a phone does not have.** No paste (neither the Paste key nor the clipboard), no
  select-all-and-replace, no multi-character insert.
- **Use the phone's tools.** Tap the syntax toolbar for the symbols it offers: `EditorSyntax-Colon`,
  `EditorSyntax-Paren`, `EditorSyntax-Bracket`, `EditorSyntax-Equals`, `EditorSyntax-Underscore` and
  `EditorSyntax-Dot`. Type the rest on the keyboard. Tap a key by its id (`wf browser tap-on --id EditorSyntax-Paren`).
  The toolbar is wider than the screen: bring a key into view first
  (`wf browser scroll-until-visible --element-id EditorSyntax-Dot --from-id EditorSyntaxToolbar --direction RIGHT`);
  a key past the screen's edge is not visible, so `tap-on` waits for it and fails.
- **Let the editor work.** Press Return for a new line and keep CodeMirror's indentation. Dedent with
  Backspace or `EditorSyntax-Dedent`. Type over an auto-closed bracket instead of adding a second one.
- **Look after every line.** Take a screenshot after each line and correct what the editor did
  differently from what you meant (indentation, a closed bracket) before going on.
- **Make one honest mistake per problem.** Type one wrong character, see it in a screenshot, and fix it
  with Backspace.

## Setup

```bash
wf browser tailnet-forward --host leetcode-backend-prod --port 8789 --local-port 8789   # use the name `wf vm net device list` shows; it was leetcode-backend-prod-2 on 2026-10-05
# The daemon holds the forward: after `wf browser stop`/`start`, open it again, or Run Local says "Failed to fetch".
# Run Local needs the credential, which is inlined into dist/: never publish or commit dist/.
EXPO_PUBLIC_DEVICE_LAB=1 EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN="$(wf secret get VM_GATEWAY_AUTH_SEED)" \
  pnpm --filter mla-tamagui run export:web
# Serve under the export's baseUrl (/mla-tamagui). Served at the root, a dynamic route such as
# /tamil/uyir-1 renders "No lesson with id [lessonId]" (dry run, 2026-10-05).
mkdir -p .agents/tmp/device-lab-serve && ln -sfn "$PWD/frontend/mla-tamagui/dist" .agents/tmp/device-lab-serve/mla-tamagui
setsid nohup node frontend/mla-tamagui/e2e/lib/staticServer.mjs .agents/tmp/device-lab-serve 8800 \
  > .agents/tmp/device-lab-serve.log 2>&1 & echo $! > .agents/tmp/device-lab-serve.pid
mkdir -p .agents/tmp/device-lab-g3
wf browser start-device --device code --screen-size iphone-15-pro
wf browser start-recording .agents/tmp/device-lab-g3/g3.mp4
```

## Steps

### Problem 1: `valid-palindrome`

Target solution, under the stub's `def isPalindrome(self, s: str) -> bool:`:

```python
        left, right = 0, len(s) - 1
        while left < right:
            if not s[left].isalnum():
                left += 1
            elif not s[right].isalnum():
                right -= 1
            elif s[left].lower() != s[right].lower():
                return False
            else:
                left += 1
                right -= 1
        return True
```

| # | Do | Expect |
| --- | --- | --- |
| 1 | `wf browser open-link http://127.0.0.1:8800/mla-tamagui/leetcode/valid-palindrome`; `wf browser assert-visible --id CodeEditorInput --timeout 30000`; screenshot (`wf browser take-screenshot .agents/tmp/device-lab-g3/<step>.png`). If the editor is not on the stub, `wf browser tap-on --id ProblemEditorResetButton`, then `wf browser tap-on --id ResetCodeDialog-Confirm` | The Python stub. The editor focuses itself, caret at the start, and the keyboard comes up, as on the phone: the WebView lets the editor raise it without a tap (`keyboardDisplayRequiresUserAction={false}`) |
| 2 | Tap the end of the stub's last line (`wf browser tap-on --css .cm-line:last-child`); screenshot | The caret in the method body and the mock keyboard up, raised by the tap |
| 3 | Write the solution line by line: words with `input-text`, the toolbar symbols by tapping them, Return with `press-key Enter`; a screenshot after every line | Each line lands where intended; CodeMirror's indentation is kept or corrected |
| 4 | The mistake: type one wrong character, screenshot, `press-key Backspace`, type the right one | The typo is visible, then gone |
| 5 | Put the keyboard away first, as Run Local sits under it: bring Done into view (`wf browser scroll-until-visible --element-id EditorAction-Done --from-id EditorActionToolbar --direction RIGHT`) and `wf browser tap-on --id EditorAction-Done`. Then `wf browser tap-on --id ProblemEditorRunLocalButton`; `wf browser assert-visible --id ProblemEditorRunOutcomeTotals --timeout 60000`; screenshot | The keyboard goes down; then "14 / 14 test cases passed" |

### Problem 2: `two-sum`

Target solution, under the stub's `def twoSum(self, nums: List[int], target: int) -> List[int]:`:

```python
        seen = {}
        for i, n in enumerate(nums):
            if target - n in seen:
                return [seen[target - n], i]
            seen[n] = i
        return []
```

| # | Do | Expect |
| --- | --- | --- |
| 6 | Steps 1–5 on `/mla-tamagui/leetcode/two-sum` | Every test passes ("N / N test cases passed") |

### Recording

| # | Do | Expect |
| --- | --- | --- |
| 7 | `wf browser stop-recording`, then `wf browser stop-device --device code`; the `.mp4`, its input log and the screenshots are in `.agents/tmp/device-lab-g3/` | The files are there |
| 8 | `wf browser check-taps .agents/tmp/device-lab-g3/g3.mp4` | A dot at every logged tap |
| 9 | Read the input log: count `type` entries that inserted more than one character, any paste, and the inter-key p50 | Zero, none, and 150–300 ms |
| 10 | Publish `.agents/tmp/device-lab-g3` (videos, screenshots, input logs) as one page | Its link; put it in the result record and the last step's notes |

## Gates

1. **All-pass.** Run Local reports every test passing on both problems.
2. **Typed like a person.** The input log shows taps, toolbar taps and single keys only: no paste, no
   replace, no multi-character insert. The inter-key p50 is within 150–300 ms.
3. **The editor did the editor's work.** At least one Return in each problem kept CodeMirror's
   auto-indent, at least one auto-closed bracket was typed over, and the typo was fixed with Backspace.
   All three are visible in the screenshots.
4. **Taps are visible.** `check-taps` passes.

The gate fails on any miss. Fix the app, the mock or the driver, then run it again; never relax a gate.

## Recording the run

Write `docs/runbooks/.results/device-lab-codemirror-typing.json` by hand: `runbook`, `sha` (the `origin/dev`
commit the export was built from), `ranAt`, `verdict` (`pass` only when every gate holds) and `evidence`
(both Run Local results, the inter-key p50, the `check-taps` result and the artifacts link). Then tick G3
in [`progress.md`](../plans/device-lab/progress.md) with the same.
