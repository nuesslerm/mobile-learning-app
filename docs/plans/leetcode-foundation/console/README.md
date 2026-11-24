# The console: reference UX from the owner's recording (section G of issue 86, 22 Nov)

Source: a 14.4 s iPhone screen recording of LeetCode's own iOS app (Python3, "Valid Parentheses"), 1284x2778, sent by the owner with "i also want this same UX for the local code submit". Four frames are kept beside this file: `reference-keyboard-up.png` (t 0.4 s), `reference-testcase-tab.png` (sheet open on Testcase), `reference-pending.png` (t 6.1 s) and `reference-run-result.png` (Run Result).

What the recording shows, in order:

1. t 0 to 2.6 s, editor focused, keyboard up. An **action bar** sits directly above the symbol toolbar and the keyboard: on the left a dark pill `Console` (terminal icon); on the right three controls: a round reset icon (circular arrow), a round `Run` (play triangle), and a green pill `Submit`. The editor above shows the code with line numbers, a highlighted current line and a caret.
2. t 2.7 s, tap `Console`: the keyboard goes down and a **rounded bottom sheet** rises over the lower half of the screen (about 55 percent of the height; the editor stays visible above it, not dimmed). The sheet has a grab handle at its top edge. Its content, top to bottom:
   - a segmented control `Testcase | Run Result` (Testcase selected, a rounded thumb that slides between the two);
   - a row of **case chips** (`Case 1` ... `Case 5`, the selected chip filled white-on-dark, the others grey), scrolling sideways (the first chip is cut at the edge when scrolled), and at the right end two round buttons: `+` (add a case) and a pencil (edit the cases);
   - the selected case's **inputs**: one field per parameter, with the parameter name as a small caption (`s`) and the value in monospace (`"([)]"`), on a slightly lighter card;
   - empty space below, then the **footer**: `Reset` (grey pill, left), `Run` (round) and `Submit` (green pill) at the right. The footer is the action bar moved into the sheet; it keeps the same three actions.
3. t 3.4 to 5.6 s: the chips are scrolled sideways and tapped: `Case 5`, then `Case 4`, `Case 3`; the input card changes to the case's value at once (`"([)]"`, `"(])"`, `"()"` ...). The chip row scrolls so the selected chip stays visible.
4. t 5.9 s, tap `Run`: a **Pending overlay**: the sheet and the editor blur and dim, and a translucent rounded card appears at the centre with an amber animation (a small window with a grid, two pulsing dots and a horizontal scan line sweeping down) and the word `Pending...` in amber below it. It lasts until the run answers (about 1.5 s here).
5. The sheet **switches to `Run Result` by itself** (the segmented thumb slides over): a red heading `Runtime Error`; a red-bordered box with the error text in monospace (`IndentationError: expected an indented block after function definition on line 60` ... and a line `Line 4  (Solution.py)`); then a caption `Last Executed Input` with a link `Open Testcase` (with an icon) at the right, and under it the input that ran (`s = "()"`). The footer is unchanged.
6. t 8.4 s: **drag the sheet down** (by the handle or its body): it follows the finger and goes away; the editor is full height again. Tapping the editor raises the keyboard and the action bar returns above it (t 9 s).
7. t 11 s, tap `Run` in the action bar **with the keyboard up**: the Pending overlay shows over the editor (the keyboard goes down), and when the run answers the sheet **rises by itself on `Run Result`** with the same error card (t 12 to 14 s). So `Run` always ends on the sheet's Run Result tab, whether the sheet was open or not.

Not in the recording (the owner asked for "the local code submit"): `Submit`. On LeetCode `Run` runs the cases in the Testcase tab (the samples plus the ones the learner added) and `Submit` runs the full set and answers with a verdict (Accepted with runtime, or Wrong Answer with `n / m testcases passed` and the first failing case's input, output and expected), shown in the same Run Result tab.

Brand colours seen: dark editor background, sheet a lighter dark grey, Submit green (about #2cbb5d), errors red (heading and border, text a salmon red), Pending amber (about #f5a623), chips and fields mid-grey on dark.
