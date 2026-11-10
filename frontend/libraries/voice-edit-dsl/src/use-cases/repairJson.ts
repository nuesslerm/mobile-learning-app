/**
 * Planner replies are JSON with code inside string literals, and models emit
 * the code's line breaks raw instead of as `\n` -- `JSON.parse` then fails on
 * "Bad control character in string literal" for an answer that is otherwise
 * exactly right. This escapes control characters that sit inside a string
 * literal and leaves everything else byte-for-byte alone, so a valid document
 * passes through unchanged and the repair is a pure function of the input.
 */
export function repairJsonControlChars(text: string): string {
  let out = '';
  let inString = false;
  let escaped = false;
  for (const ch of text) {
    if (inString) {
      if (escaped) {
        out += ch;
        escaped = false;
        continue;
      }
      if (ch === '\\') {
        out += ch;
        escaped = true;
        continue;
      }
      if (ch === '"') {
        inString = false;
        out += ch;
        continue;
      }
      const code = ch.charCodeAt(0);
      if (code < 0x20) {
        out +=
          code === 0x0a
            ? '\\n'
            : code === 0x0d
              ? '\\r'
              : code === 0x09
                ? '\\t'
                : `\\u${code.toString(16).padStart(4, '0')}`;
        continue;
      }
      out += ch;
      continue;
    }
    if (ch === '"') inString = true;
    out += ch;
  }
  return out;
}

/** `JSON.parse` that tolerates raw line breaks inside strings; throws the original error otherwise. */
export function parseLenientJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch (err) {
    const repaired = repairJsonControlChars(text);
    if (repaired === text) throw err;
    return JSON.parse(repaired);
  }
}
