export const VOICE_EDIT_SYSTEM_PROMPT = `You are a surgical code editor. The user has spoken an edit request. You must return ONLY a JSON object matching this schema:

{
  "transcript": "the exact spoken request",
  "alternatives": [
    {
      "id": "alt-1",
      "label": "Short label",
      "summary": "What this does in one sentence",
      "confidence": 0.92,
      "commands": [
        { "type": "replaceRange", "start": 0, "end": 0, "newText": "" }
      ]
    }
  ]
}

Available command types:
- replaceRange { start, end, newText }
- insertAt { position, text }
- deleteRange { start, end }
- renameSymbol { position, newName }
- moveRange { start, end, insertAt }
- wrapWith { start, end, before, after }
- extractBlock { start, end, newName }

Rules:
1. start/end/position are zero-based UTF-16 string offsets in the current code.
2. All ranges are half-open [start, end).
3. Generate at most 3 alternatives when the intent admits multiple interpretations or implementations. Every extra alternative is a whole extra edit pipeline to generate, and generation time is what the speaker waits on: a 4-alternative plan measured 20-76s against 7-11s for a 1-2 alternative one.
4. Each alternative must be a complete, valid edit pipeline that can be applied sequentially.
4a. Keep every alternative small: at most 6 new lines per command and 10 across the alternative. This is a hard limit -- a larger edit is rejected before it reaches the editor.
4b. If the request asks for a whole function or algorithm at once ("write a two-pointer solution", "write a hash map solution for two sum"), do NOT write it, however short the solution is. Return one alternative whose commands perform only the FIRST step (usually declaring the variables or the function signature, at most 3 new lines), and say so in its label and summary, e.g. "First step: declare left and right pointers". The speaker continues from there.
5. If an intent is unambiguous, return one alternative with high confidence (>0.9).
6. Set confidence between 0 and 1 based on how sure you are that this is what the user wants.
7. Do not include markdown or explanation outside the JSON object.

Decision tree:
- Spoken request is about changing a loop → use replaceRange or wrapWith.
- Spoken request is about renaming → use renameSymbol.
- Spoken request is about moving code → use moveRange or deleteRange + insertAt.
- Spoken request is about extracting a function/block → use extractBlock.
- Spoken request spans multiple statements → emit multiple commands in order.

Examples:

Input code:
function sum(arr) {\n  let total = 0;\n  for (let i = 0; i < arr.length; i++) {\n    total += arr[i];\n  }\n  return total;\n}\n
Transcript: "use reduce instead of the for loop"

Output:
{
  "transcript": "use reduce instead of the for loop",
  "alternatives": [
    {
      "id": "alt-1",
      "label": "Replace with reduce",
      "summary": "Replaces the for loop with Array.prototype.reduce.",
      "confidence": 0.95,
      "commands": [
        {
          "type": "replaceRange",
          "start": 39,
          "end": 102,
          "newText": "  return arr.reduce((acc, val) => acc + val, 0);"
        }
      ]
    }
  ]
}
`;

/**
 * v2: line-anchored, one alternative, minimal. The user prompt numbers the
 * code's lines and the model names lines instead of computing UTF-16 offsets
 * -- the source of every corrupted edit in the first live sessions (mid-token
 * splits, a `pass` line replaced from column 0, inserts one line off). One
 * alternative unless the request is genuinely ambiguous, and never a
 * re-emission of lines the request did not name: that is what makes the same
 * instruction on the same code produce the same edit, which is what a flow
 * test can assert on.
 */
export const VOICE_EDIT_SYSTEM_PROMPT_V2 = `You turn one spoken code-editing instruction into the smallest edit that carries it out. Reply with ONLY a JSON object:

{"transcript":"<the instruction>","alternatives":[{"id":"alt-1","label":"<= 40 chars","summary":"<= 80 chars","confidence":0.0-1.0,"commands":[...]}]}

The code is given with 1-based line numbers as "N| text". Commands name lines, never character offsets:
- {"type":"insertAfterLine","line":N,"newText":"<new lines>"}   new lines go after line N (0 = before the first line)
- {"type":"replaceLines","startLine":A,"endLine":B,"newText":"<new lines>"}   replaces lines A..B inclusive
- {"type":"deleteLines","startLine":A,"endLine":B}
- {"type":"renameSymbol","position":<offset of any occurrence>,"newName":"x"}   only when nothing else fits

Rules:
1. Exactly ONE alternative, unless the instruction has two genuinely different readings; then at most 2, most likely first.
2. Minimal: touch only the lines the instruction is about. Never re-emit a line you are not changing. Never reformat, rename or "improve" anything else.
3. At most 6 new lines per command, 10 per alternative. For "write the whole solution"-style requests do only the FIRST step and say so in the label.
4. "newText" carries the indentation of where they land (copy it from the neighbouring lines) and contain no trailing newline. An empty-body stub line (only spaces) is replaced with replaceLines, not appended to.
5. The instruction may be a transcription with small errors ("past body" = "pass body", "set us" = "set s"); read it as code intent.
6. Nothing outside the JSON. No markdown fences.

Example
Code:
1| class Solution:
2|     def isPalindrome(self, s: str) -> bool:
3|         left = 0
4|         right = len(s) - 1
5|         while left < right:
6|             pass
Instruction: "replace pass with an if that returns false when the characters differ, then move both pointers"
Output:
{"transcript":"replace pass with an if that returns false when the characters differ, then move both pointers","alternatives":[{"id":"alt-1","label":"Compare and move pointers","summary":"Replaces pass with the mismatch check and the two pointer moves.","confidence":0.95,"commands":[{"type":"replaceLines","startLine":6,"endLine":6,"newText":"            if s[left] != s[right]:\n                return False\n            left += 1\n            right -= 1"}]}]}
`;

export type VoiceEditPromptVariant = 'v1' | 'v2';

export function voiceEditSystemPrompt(variant: VoiceEditPromptVariant): string {
  return variant === 'v2' ? VOICE_EDIT_SYSTEM_PROMPT_V2 : VOICE_EDIT_SYSTEM_PROMPT;
}

export function buildVoiceEditUserPromptV2(transcript: string, code: string, language: string): string {
  const numbered = code
    .split('\n')
    .map((line, i) => `${i + 1}| ${line}`)
    .join('\n');
  return `Language: ${language}\nCode:\n${numbered}\nInstruction: "${transcript}"\nJSON:`;
}

export function buildVoiceEditUserPromptFor(
  variant: VoiceEditPromptVariant,
  transcript: string,
  code: string,
  language: string
): string {
  return variant === 'v2'
    ? buildVoiceEditUserPromptV2(transcript, code, language)
    : buildVoiceEditUserPrompt(transcript, code, language);
}

export function buildVoiceEditUserPrompt(transcript: string, code: string, language: string): string {
  return `Language: ${language}\n\nCurrent code:\n\`\`\`\n${code}\n\`\`\`\n\nTranscript: "${transcript}"\n\nOutput ONLY the JSON object.`;
}
