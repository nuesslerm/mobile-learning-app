import type { EditAlternative, EditCommand } from '../contracts/schema';

import { lineStarts, resolveLineCommands } from './lineCommands';

/**
 * Pseudo-code mode: think out loud into the buffer, then turn one line at a
 * time into real code.
 *
 * Everything but the "realise" step is resolved here, without a model, so it
 * is immediate and the same words always land the same line: a spoken step
 * becomes an indented comment at the end of the method, and the mode itself
 * is switched by a spoken command. Only turning a pseudo line into code needs
 * the planner, and that call is fenced to the line(s) named -- an answer that
 * touches anything else is marked invalid by `restrictAlternativeToLines`.
 */
export type PseudoCommand =
  | { kind: 'enter' }
  | { kind: 'exit' }
  | { kind: 'note'; alternative: EditAlternative }
  | { kind: 'realize'; request: RealizeRequest };

export type RealizeRequest = {
  /** 1-based, inclusive: the pseudo lines to be replaced by code. */
  startLine: number;
  endLine: number;
  /** The instruction the planner receives instead of the raw transcript. */
  transcript: string;
  label: string;
};

const PSEUDO = String.raw`(?:pseudo|sudo|pseudo-|pseudo )\s?code`;
const ENTER_RE = new RegExp(
  String.raw`\b(?:enter|start|begin|open|switch\s+to|go\s+(?:in)?to|turn\s+on)\s+(?:the\s+)?${PSEUDO}(?:\s+mode)?\b|\b${PSEUDO}\s+mode\s+on\b`,
  'i'
);
const EXIT_RE = new RegExp(
  String.raw`\b(?:exit|leave|stop|end|quit|close|turn\s+off)\s+(?:the\s+)?${PSEUDO}(?:\s+mode)?\b|\b${PSEUDO}\s+mode\s+off\b`,
  'i'
);
const REALIZE_VERB = String.raw`(?:realise|realize|implement|expand|convert|translate|turn|make|code\s+up|write\s+out)`;
const REALIZE_RE = new RegExp(
  String.raw`\b${REALIZE_VERB}\b[\s\S]*?\b(?:${PSEUDO}|comment|step|note|line|lines)\b`,
  'i'
);
const ALL_RE = new RegExp(
  String.raw`\b(?:all|every|whole|entire|everything|the\s+${PSEUDO}(?:\s+into|\s+to|\s+as|\s*$))`,
  'i'
);
const RANGE_RE =
  /\blines?\s+(\w+)(?:\s*(?:to|through|thru|until|-|–)\s*(\w+))?/i;

/** Characters an offset command may overshoot the fenced lines by and still count as drift. */
const OFFSET_DRIFT_CHARS = 3;

const NUMBER_WORDS: Record<string, number> = {
  one: 1,
  two: 2,
  three: 3,
  four: 4,
  five: 5,
  six: 6,
  seven: 7,
  eight: 8,
  nine: 9,
  ten: 10,
  eleven: 11,
  twelve: 12,
  thirteen: 13,
  fourteen: 14,
  fifteen: 15,
  sixteen: 16,
  seventeen: 17,
  eighteen: 18,
  nineteen: 19,
  twenty: 20,
  first: 1,
  second: 2,
  third: 3,
  fourth: 4,
  fifth: 5,
};

function parseLineNumber(word: string | undefined): number | null {
  if (!word) return null;
  const digits = Number(word);
  if (Number.isInteger(digits) && digits > 0) return digits;
  return NUMBER_WORDS[word.toLowerCase()] ?? null;
}

export function commentPrefix(language: string): string {
  const lang = language.toLowerCase();
  return lang.startsWith('python') ||
    lang === 'ruby' ||
    lang === 'shell' ||
    lang === 'bash'
    ? '#'
    : '//';
}

/** 1-based numbers of the lines that are comments in `language`. */
export function pseudoLines(code: string, language: string): number[] {
  const prefix = commentPrefix(language);
  const result: number[] = [];
  code.split('\n').forEach((line, i) => {
    if (line.trim().startsWith(prefix)) result.push(i + 1);
  });
  return result;
}

function indentOf(line: string): string {
  return /^\s*/.exec(line)?.[0] ?? '';
}

function cleanNote(text: string): string {
  return text
    .trim()
    .replace(/[.!?]+$/, '')
    .trim();
}

/** 0-based index of the last non-blank line; 0 when the buffer is blank. */
export function lastNonBlankLineIndex(code: string): number {
  const lines = code.split('\n');
  let last = lines.length - 1;
  while (last > 0 && lines[last]!.trim() === '') last -= 1;
  return last;
}

/** 1-based line of a whitespace-only final stub line, else null. */
export function trailingStubLine(code: string): number | null {
  const lines = code.split('\n');
  const last = lastNonBlankLineIndex(code);
  return lines.length - 1 > last &&
    lines[lines.length - 1]!.trim() === '' &&
    lines[lines.length - 1]!.length > 0
    ? lines.length
    : null;
}

/**
 * The next pseudo line goes under the last non-blank line: one level deeper
 * when that line opens a block (`:` / `{`), else at its own indentation. A
 * whitespace-only stub line (the empty method body every starter has) is
 * replaced rather than appended to, so the first note lands as the body.
 */
export function buildPseudoNote(
  code: string,
  text: string,
  language: string
): EditAlternative {
  const prefix = commentPrefix(language);
  const note = cleanNote(text);
  const lines = code.split('\n');
  const last = lastNonBlankLineIndex(code);
  const anchor = lines[last] ?? '';
  const trailingStub = trailingStubLine(code);
  const opensBlock = /[:{]\s*$/.test(anchor);
  const indent = opensBlock ? `${indentOf(anchor)}    ` : indentOf(anchor);
  const commands: EditCommand[] =
    trailingStub !== null
      ? [
          {
            type: 'replaceLines',
            startLine: trailingStub,
            endLine: trailingStub,
            newText: `${indentOf(lines[trailingStub - 1]) || indent}${prefix} ${note}`,
          },
        ]
      : [
          {
            type: 'insertAfterLine',
            line: last + 1,
            text: `${indent}${prefix} ${note}`,
          },
        ];
  return {
    id: 'local-pseudo-note',
    label: `Pseudo: ${note.length > 34 ? `${note.slice(0, 31)}…` : note}`,
    summary: `Adds "${prefix} ${note}" as the next pseudo-code line. Resolved locally, without the planner.`,
    confidence: 0.99,
    valid: true,
    commands,
  };
}

/** The instruction the planner gets for turning `startLine..endLine` into code. */
export function buildRealizeRequest(
  code: string,
  language: string,
  target: 'all' | { startLine: number; endLine?: number }
): RealizeRequest | null {
  const comments = pseudoLines(code, language);
  if (comments.length === 0) return null;
  let startLine: number;
  let endLine: number;
  if (target === 'all') {
    startLine = comments[0]!;
    endLine = comments[comments.length - 1]!;
  } else {
    startLine = target.startLine;
    endLine = target.endLine ?? target.startLine;
    if (endLine < startLine) [startLine, endLine] = [endLine, startLine];
    // Only comment lines can be realised; naming a code line is a different edit.
    const named = [];
    for (let n = startLine; n <= endLine; n += 1) named.push(n);
    if (!named.some((n) => comments.includes(n))) return null;
  }
  const lines = code.split('\n');
  const prefix = commentPrefix(language);
  const texts = [];
  for (let n = startLine; n <= endLine; n += 1) {
    const line = lines[n - 1] ?? '';
    if (line.trim().startsWith(prefix))
      texts.push(`line ${n}: "${line.trim().slice(prefix.length).trim()}"`);
  }
  const where =
    startLine === endLine
      ? `line ${startLine}`
      : `lines ${startLine} to ${endLine}`;
  const transcript =
    `Replace the pseudo-code comment${startLine === endLine ? '' : 's'} on ${where} (${texts.join('; ')}) ` +
    `with the real ${language} code it describes, using replaceLines on exactly ${where}. ` +
    `Keep the indentation of ${where}. Do not add, remove or change any other line.`;
  return { startLine, endLine, transcript, label: `Implement ${where}` };
}

/**
 * Spoken pseudo-code commands, or null when the words are an ordinary edit.
 * In pseudo mode every non-command utterance is a note; outside it only the
 * mode switches and "realise" are recognised.
 */
export function parsePseudoCommand(
  transcript: string,
  code: string,
  language: string,
  options: { pseudoMode: boolean }
): PseudoCommand | null {
  const text = transcript.trim();
  if (text === '') return null;
  if (EXIT_RE.test(text)) return { kind: 'exit' };
  if (ENTER_RE.test(text)) return { kind: 'enter' };
  if (REALIZE_RE.test(text)) {
    const range = RANGE_RE.exec(text);
    const start = parseLineNumber(range?.[1]);
    const target =
      start !== null
        ? {
            startLine: start,
            ...(parseLineNumber(range?.[2]) !== null
              ? { endLine: parseLineNumber(range?.[2])! }
              : {}),
          }
        : ALL_RE.test(text) ||
            /\b(?:the|this|that|my)\s+(?:pseudo|sudo)/i.test(text)
          ? 'all'
          : null;
    if (target !== null) {
      const request = buildRealizeRequest(code, language, target);
      if (request) return { kind: 'realize', request };
    }
    if (!options.pseudoMode) return null;
  }
  if (options.pseudoMode)
    return { kind: 'note', alternative: buildPseudoNote(code, text, language) };
  return null;
}

/**
 * A realised alternative may only touch the lines it was asked about: every
 * command, resolved to offsets, has to sit inside [start of startLine, end of
 * endLine]. Anything else -- a rewrite of the neighbouring code, a rename --
 * is marked invalid so the picker greys it out instead of applying it.
 *
 * An in-fence answer is then normalised to `replaceLines` over exactly the
 * fenced lines. The planner was asked to replace those lines and nothing
 * else, so its offsets carry no information the fence does not already hold
 * -- and offset planners are off by one at either edge often enough that
 * applying them verbatim glued a pseudo line onto its neighbour live. The
 * indentation of the first fenced line is restored on any line the answer
 * left flush.
 *
 * With `mode: 'insertAfter'` the fence is the anchor line plus the position
 * right after its line break: only an insertion is allowed, and it is
 * normalised to `insertAfterLine` at the anchor line. An answer that rewrites
 * the anchor line itself is invalid.
 */
export function restrictAlternativeToLines(
  alternative: EditAlternative,
  code: string,
  lines: { startLine: number; endLine: number },
  options?: { mode?: 'replace' | 'insertAfter' }
): EditAlternative {
  const mode = options?.mode ?? 'replace';
  const starts = lineStarts(code);
  const lineFrom = starts[Math.min(lines.startLine, starts.length) - 1] ?? 0;
  const lineTo =
    lines.endLine < starts.length ? starts[lines.endLine] : code.length;
  // Offset planners routinely take the line break that ends the previous line
  // (or the one after) into the span they replace; that still edits only
  // these lines, so one newline of slack is allowed at either edge.
  const from =
    lineFrom > 0 && code[lineFrom - 1] === '\n' ? lineFrom - 1 : lineFrom;
  const to = code[lineTo] === '\n' ? lineTo + 1 : lineTo;
  let resolved;
  try {
    resolved = resolveLineCommands(code, alternative.commands);
  } catch (err) {
    return {
      ...alternative,
      valid: false,
      invalidReason: err instanceof Error ? err.message : String(err),
    };
  }
  const where =
    lines.startLine === lines.endLine
      ? `line ${lines.startLine}`
      : `lines ${lines.startLine}-${lines.endLine}`;
  for (const command of resolved) {
    if (command.type === 'renameSymbol') {
      return {
        ...alternative,
        valid: false,
        invalidReason: `A rename is not an implementation of ${where}.`,
      };
    }
    // The executor clamps offsets to the buffer before applying, so an
    // end one past the last character is the same edit; judge the clamped span.
    const raw =
      command.type === 'insertAt'
        ? { start: command.position, end: command.position }
        : { start: command.start, end: command.end };
    const span = {
      start: Math.max(0, Math.min(raw.start, code.length)),
      end: Math.max(0, Math.min(raw.end, code.length)),
    };
    if (mode === 'insertAfter' && span.start !== span.end) {
      return {
        ...alternative,
        valid: false,
        invalidReason: `Inserting after ${where} must not rewrite it.`,
      };
    }
    // Offset planners drift by a character or two at either edge (seen live:
    // a span starting two characters before the fenced line). A short
    // overshoot on a span that is otherwise this line is drift, not intent,
    // and the normalisation below discards the offsets anyway. A span that
    // reaches further, or one that does not touch the line at all, is an edit
    // to something else.
    const overshoot = Math.max(from - span.start, span.end - to, 0);
    const touches = span.end >= lineFrom && span.start <= lineTo;
    if (overshoot > OFFSET_DRIFT_CHARS || !touches) {
      return {
        ...alternative,
        valid: false,
        invalidReason: `Edit reaches outside ${where}.`,
      };
    }
  }
  const single = resolved.length === 1 ? resolved[0] : null;
  const replacement =
    single?.type === 'replaceRange'
      ? single.newText
      : single?.type === 'insertAt'
        ? single.text
        : null;
  if (replacement === null) return alternative;
  const indent = indentOf(code.split('\n')[lines.startLine - 1] ?? '');
  const newText = replacement
    .replace(/^\n+/, '')
    .replace(/\n+$/, '')
    .split('\n')
    .map((line) =>
      line.trim() === '' || /^\s/.test(line) ? line : `${indent}${line}`
    )
    .join('\n');
  if (newText.trim() === '') return alternative;
  if (mode === 'insertAfter') {
    return {
      ...alternative,
      commands: [
        {
          type: 'insertAfterLine',
          line: lines.startLine,
          text: newText,
        },
      ],
    };
  }
  return {
    ...alternative,
    commands: [
      {
        type: 'replaceLines',
        startLine: lines.startLine,
        endLine: lines.endLine,
        newText,
      },
    ],
  };
}
