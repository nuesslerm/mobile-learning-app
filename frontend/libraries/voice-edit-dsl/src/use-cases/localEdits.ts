import type { EditAlternative } from '../contracts/schema';

/**
 * Edits that can be resolved from the transcript alone, without an LLM.
 *
 * A rename is mechanical: the transcript names the old symbol and the new one,
 * and the buffer says where it is. Sending it to the planner buys nothing and
 * costs the whole planning round trip -- measured against the deployed planner,
 * rename requests came back in 6.8s, 8.8s and 11.2s, and the same planner took
 * 20.5-75.7s on larger requests. Resolving it here is immediate.
 *
 * Deliberately narrow. This is a fast path, not a parser: anything that is not
 * unmistakably a rename returns null and goes to the planner as before, because
 * a wrong local guess is far worse than a slow correct answer.
 */
const RENAME_PATTERNS: RegExp[] = [
  // "rename the variable left to low", "rename left to low everywhere".
  // `re name` / `we name` are there because that is what whisper actually
  // produces: a live run of "rename the variable right to high everywhere"
  // came back as "We name the variable right to high everywhere", which the
  // strict spelling missed -- and a fast path that misses is just latency.
  /\b(?:rename|re[- ]name|we name)\s+(?:the\s+)?(?:variable\s+|symbol\s+|function\s+)?["']?([A-Za-z_][A-Za-z0-9_]*(?:\s+[a-z]+){0,2})["']?\s+to\s+["']?([A-Za-z_][A-Za-z0-9_]*)["']?/i,
  // "call the variable left low instead"
  /\bcall\s+(?:the\s+)?(?:variable\s+|symbol\s+)?["']?([A-Za-z_][A-Za-z0-9_]*)["']?\s+["']?([A-Za-z_][A-Za-z0-9_]*)["']?\s+instead\b/i,
];

/** Offset of `name` as a whole word in `code`, or -1. */
function findIdentifier(code: string, name: string): number {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = new RegExp(`\\b${escaped}\\b`).exec(code);
  return match ? match.index : -1;
}

const NOT_RENAMEABLE = new Set([
  'and', 'as', 'assert', 'async', 'await', 'break', 'class', 'continue', 'def', 'del', 'elif', 'else',
  'except', 'finally', 'for', 'from', 'global', 'if', 'import', 'in', 'is', 'lambda', 'nonlocal', 'not',
  'or', 'pass', 'raise', 'return', 'try', 'while', 'with', 'yield', 'True', 'False', 'None', 'self',
  'len', 'range', 'enumerate', 'print', 'str', 'int', 'float', 'list', 'dict', 'set', 'tuple', 'bool',
  'max', 'min', 'sum', 'abs', 'sorted', 'reversed', 'zip', 'map', 'filter', 'any', 'all', 'List', 'Dict',
  'Optional',
]);

/** A rough English sound key, so "scene" meets `seen`: whisper spells a homophone as the commoner word. */
export function soundKey(word: string): string {
  const w = word
    .toLowerCase()
    .replace(/[^a-z]/g, '')
    .replace(/^kn/, 'n')
    .replace(/^wr/, 'r')
    .replace(/^wh/, 'w')
    .replace(/sch/g, 'sk')
    .replace(/sc(?=[eiy])/g, 's')
    .replace(/igh/g, 'i')
    .replace(/ph/g, 'f')
    .replace(/ck/g, 'k')
    .replace(/qu/g, 'kw')
    .replace(/c(?=[eiy])/g, 's')
    .replace(/[cq]/g, 'k')
    .replace(/x/g, 'ks')
    .replace(/z/g, 's');
  if (!w) return '';
  const rest = w.slice(1).replace(/[aeiouyh]/g, '');
  return (w[0] + rest).replace(/(.)\1+/g, '$1');
}

function editDistance(a: string, b: string): number {
  const row = Array.from({ length: b.length + 1 }, (_, j) => j);
  for (let i = 1; i <= a.length; i++) {
    let diagonal = row[0];
    row[0] = i;
    for (let j = 1; j <= b.length; j++) {
      const above = row[j];
      row[j] = Math.min(row[j] + 1, row[j - 1] + 1, diagonal + (a[i - 1] === b[j - 1] ? 0 : 1));
      diagonal = above;
    }
  }
  return row[b.length];
}

/** The one buffer name that sounds like `heard`, within two edits; ambiguity goes to the planner. */
function soundAlikeIdentifier(code: string, heard: string): string | null {
  const key = soundKey(heard);
  const lower = heard.toLowerCase();
  const names = new Set(code.match(/\b[A-Za-z_][A-Za-z0-9_]*\b/g) ?? []);
  const matches = [...names].filter(
    (name) => !NOT_RENAMEABLE.has(name) && soundKey(name) === key && editDistance(name.toLowerCase(), lower) <= 2
  );
  return matches.length === 1 ? matches[0] : null;
}

const TRAILING_FILLER = new Set(['everywhere', 'instead', 'please', 'now', 'too', 'again', 'there', 'here', 'throughout']);

/** Whisper splits compounds ("lookup" comes back as "look up"): one bare word closing the sentence belongs to the name. */
function joinedTail(transcript: string, match: RegExpExecArray): string {
  if (/["']$/.test(match[0])) return '';
  const tail = /^\s+([a-z]+)\s*[.!?]?\s*$/.exec(transcript.slice(match.index + match[0].length));
  return tail && !TRAILING_FILLER.has(tail[1]) ? tail[1] : '';
}

/** The buffer name a heard phrase stands for: "max area" is `max_area` or `maxArea`; one word may be a homophone. */
function spokenIdentifier(code: string, heard: string): string | null {
  const words = heard.trim().split(/\s+/);
  if (words.length === 1) return findIdentifier(code, heard) !== -1 ? heard : soundAlikeIdentifier(code, heard);
  const camel = words[0] + words.slice(1).map((w) => w[0].toUpperCase() + w.slice(1)).join('');
  return [words.join('_'), camel, words.join('')].find((name) => findIdentifier(code, name) !== -1) ?? null;
}

export function parseLocalEdit(
  transcript: string,
  code: string
): EditAlternative | null {
  for (const pattern of RENAME_PATTERNS) {
    const match = pattern.exec(transcript);
    if (!match) continue;
    const [, heardName, spokenName] = match;
    if (!heardName || !spokenName) continue;
    const newName = spokenName + joinedTail(transcript, match);
    // The symbol has to be in the buffer, or the fast path is a silent no-op; a homophone of it counts.
    const oldName = spokenIdentifier(code, heardName);
    if (!oldName || oldName === newName) continue;
    const position = findIdentifier(code, oldName);

    return {
      id: 'local-rename',
      label: `Rename ${oldName} to ${newName}`,
      summary: `Renames every occurrence of \`${oldName}\` to \`${newName}\`. Resolved locally, without the planner.`,
      confidence: 0.99,
      commands: [{ type: 'renameSymbol', position, newName }],
      valid: true,
    };
  }
  return null;
}
