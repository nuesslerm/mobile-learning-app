export type Wire = null | boolean | number | string | Wire[] | { [key: string]: Wire };

export const APPROX_TOLERANCE = 1e-5;
export const UNPRINTABLE = '<unprintable>';

const MAX_SAFE = BigInt(Number.MAX_SAFE_INTEGER);
const MIN_SAFE = BigInt(Number.MIN_SAFE_INTEGER);

class Cyclic extends Error {}

function shown(value: unknown): string {
  try {
    return String(value);
  } catch {
    return UNPRINTABLE;
  }
}

function isPlain(value: object): boolean {
  const prototype: unknown = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function convert(value: unknown, path: object[]): Wire {
  if (value === undefined || value === null) return null;
  switch (typeof value) {
    case 'boolean':
    case 'string':
      return value;
    case 'number':
      return Number.isFinite(value) ? value : String(value);
    case 'bigint':
      return value >= MIN_SAFE && value <= MAX_SAFE ? Number(value) : value.toString();
    case 'object':
      break;
    default:
      return shown(value);
  }
  const object = value as object;
  if (!Array.isArray(object) && !isPlain(object)) return shown(object);
  if (path.includes(object)) throw new Cyclic();
  path.push(object);
  try {
    if (Array.isArray(object)) return Array.from(object as unknown[], (item) => convert(item, path));
    const record = object as Record<string, unknown>;
    const out: { [key: string]: Wire } = {};
    for (const key of Object.keys(record)) out[key] = convert(record[key], path);
    return out;
  } finally {
    path.pop();
  }
}

/** The JSON-clean form of a value the way a solution returned it: what a row shows and what a comparison reads. */
export function toWire(value: unknown): Wire {
  try {
    return convert(value, []);
  } catch {
    return shown(value);
  }
}

/** JSON with every object's keys in sorted order, so two equal values print the same. */
export function canonicalJson(value: Wire): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key] ?? null)}`)
      .join(',')}}`;
  }
  return JSON.stringify(value);
}

const rank = (value: Wire): number => (value === null ? 0 : typeof value === 'boolean' ? 1 : typeof value === 'number' ? 2 : typeof value === 'string' ? 3 : 4);

function order(a: Wire, b: Wire): number {
  const byType = rank(a) - rank(b);
  if (byType !== 0) return byType;
  if (typeof a === 'number' && typeof b === 'number') return a - b;
  const left = typeof a === 'string' ? a : canonicalJson(a);
  const right = typeof b === 'string' ? b : canonicalJson(b);
  return left < right ? -1 : left > right ? 1 : 0;
}

function approx(actual: Wire, expected: Wire): boolean {
  if (typeof actual === 'boolean' || typeof expected === 'boolean') return actual === expected;
  if (typeof actual === 'number' && typeof expected === 'number') return Math.abs(actual - expected) <= APPROX_TOLERANCE;
  if (Array.isArray(actual) && Array.isArray(expected)) return actual.length === expected.length && actual.every((item, i) => approx(item, expected[i] ?? null));
  return canonicalJson(actual) === canonicalJson(expected);
}

function sameMultiset(actual: Wire[], expected: Wire[]): boolean {
  const left = actual.map(canonicalJson).sort();
  const right = expected.map(canonicalJson).sort();
  return left.length === right.length && left.every((text, i) => text === right[i]);
}

function sortedInner(list: Wire[]): Wire[] | null {
  const out: Wire[] = [];
  for (const inner of list) {
    if (!Array.isArray(inner)) return null;
    out.push([...inner].sort(order));
  }
  return out;
}

/** The judge's comparison: `exact` is canonical JSON, `sorted` a multiset, `setOfSorted` each inner list sorted with the outer order kept, `approx` within 1e-5. Never throws. */
export function compareValues(actual: unknown, expected: unknown, mode?: string | null): boolean {
  const a = toWire(actual);
  const e = toWire(expected);
  const exact = () => canonicalJson(a) === canonicalJson(e);
  try {
    if (mode === 'approx') return approx(a, e);
    if (mode === 'sorted') return Array.isArray(a) && Array.isArray(e) ? sameMultiset(a, e) : exact();
    if (mode === 'setOfSorted') {
      const left = Array.isArray(a) ? sortedInner(a) : null;
      const right = Array.isArray(e) ? sortedInner(e) : null;
      return left && right ? canonicalJson(left) === canonicalJson(right) : exact();
    }
    return exact();
  } catch {
    return Object.is(a, e);
  }
}
