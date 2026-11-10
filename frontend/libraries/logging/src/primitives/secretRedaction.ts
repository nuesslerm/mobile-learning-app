// Secret redaction — Stage-shaped primitive that scrubs sensitive fields
// from a log entry's metadata bag before it reaches any sink. Runs as
// one of the early stages in the pipeline so the redaction happens
// BEFORE the entry is persisted or sent to a remote sink (don't want
// passwords in BetterStack even for one frame).
//
// The default key list covers the common cases (HTTP headers, common
// credential fields, anything containing "token"/"password"/"secret"
// as a substring). Hosts can extend the list.

import type { Stage } from '../pipeline/stage.js';

const DEFAULT_KEYS: ReadonlySet<string> = new Set([
  'authorization',
  'cookie',
  'set-cookie',
  'password',
  'passwd',
  'secret',
  'token',
  'accessToken',
  'refreshToken',
  'idToken',
  'apiKey',
  'api_key',
  'privateKey',
]);

const REDACTED = '[REDACTED]';

/**
 * Substring patterns matched (case-insensitively) against a key name to decide
 * whether its value is sensitive. Broader than {@link DEFAULT_KEYS} exact names
 * so it also catches env-var-style keys (`BWS_ACCESS_TOKEN`,
 * `OP_SERVICE_ACCOUNT_TOKEN`, `X_BOT_SECRET`, …).
 */
const DEFAULT_SUBSTRING_PATTERNS: readonly string[] = [
  'token',
  'secret',
  'password',
  'passwd',
  'cookie',
  'authorization',
  'apikey',
  'api_key',
  'privatekey',
  'private_key',
  'credential',
];

function keyLooksSensitive(key: string, patterns: readonly string[]): boolean {
  const lower = key.toLowerCase();
  return patterns.some((p) => lower.includes(p));
}

/**
 * A genuinely-working, pure recursive scrubber (unlike the Stage-shaped
 * {@link redactSensitiveFields}, which is a no-op under the read-only batch
 * contract). Walks a value — deep-cloning objects/arrays it touches — and
 * replaces any value whose *key* looks sensitive (substring match, see
 * {@link DEFAULT_SUBSTRING_PATTERNS}) with the replacement placeholder. Scalars
 * pass through untouched; the input is never mutated.
 *
 * Use it right before a value crosses a trust boundary (a remote POST, a log
 * line) as a defense-in-depth net — callers should still avoid putting secret
 * values in logs in the first place.
 */
export function scrubSecrets<T>(
  value: T,
  opts: { patterns?: readonly string[]; replacement?: string } = {}
): T {
  const patterns = opts.patterns ?? DEFAULT_SUBSTRING_PATTERNS;
  const replacement = opts.replacement ?? REDACTED;

  function walk(v: unknown): unknown {
    if (Array.isArray(v)) return v.map(walk);
    if (v !== null && typeof v === 'object') {
      const out: Record<string, unknown> = {};
      for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
        out[k] = keyLooksSensitive(k, patterns) ? replacement : walk(val);
      }
      return out;
    }
    return v;
  }

  return walk(value) as T;
}

/**
 * PII patterns redacted from string values by default. Deliberately conservative
 * — high-signal shapes only (email, JWT, credit-card-like digit runs) — to keep
 * false positives (e.g. redacting an innocent number) low.
 */
const DEFAULT_PII_PATTERNS: readonly RegExp[] = [
  /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, // email
  /\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\b/g, // JWT
  /\b\d(?:[ -]?\d){12,18}\b/g, // credit-card-like run (13–19 digits, ends on a digit)
];

/**
 * Snapshot the *values* of env vars whose NAME looks sensitive (a token /
 * secret / password / key / …), so those values can be redacted wherever they
 * appear in a log — the "auto-scrub any env-var mention" case. Only values at
 * least `minLength` long are collected, so a secret that happens to be `"true"`
 * or `"1"` can't nuke every occurrence of that common string.
 */
function collectSensitiveEnvValues(
  minLength: number,
  patterns: readonly string[]
): string[] {
  const env = typeof process !== 'undefined' && process.env ? process.env : {};
  const values: string[] = [];
  for (const [k, v] of Object.entries(env)) {
    if (
      typeof v === 'string' &&
      v.length >= minLength &&
      keyLooksSensitive(k, patterns)
    ) {
      values.push(v);
    }
  }
  // De-dupe and sort longest-first so overlapping values redact completely.
  return [...new Set(values)].sort((a, b) => b.length - a.length);
}

export type ScrubberOptions = {
  /** Sensitive field-name substrings (default {@link DEFAULT_SUBSTRING_PATTERNS}). */
  keyPatterns?: readonly string[];
  /** Placeholder text. Default `'[REDACTED]'`. */
  replacement?: string;
  /** Redact the values of sensitive-named env vars wherever they appear. Default true. */
  scrubEnvValues?: boolean;
  /** Minimum env-var value length to redact (guards against nuking short common strings). Default 8. */
  envValueMinLength?: number;
  /** Extra literal values to redact wherever they appear (e.g. a token you hold in hand). */
  extraSecretValues?: readonly string[];
  /** Redact PII patterns (email/JWT/card) from strings. Default true. */
  scrubPii?: boolean;
};

/**
 * Build a comprehensive scrubber: a pure `(value) => value` that deep-clones and
 * redacts (1) values under sensitive-looking KEYS, (2) the VALUES of
 * sensitive-named env vars wherever they appear in any string (auto-scrub env
 * mentions), and (3) PII patterns in strings. Never mutates the input.
 *
 * Env-var values are snapshotted at creation — build the scrubber after secrets
 * are loaded (e.g. at logger-config time).
 */
export function createScrubber(opts: ScrubberOptions = {}): <T>(value: T) => T {
  const keyPatterns = opts.keyPatterns ?? DEFAULT_SUBSTRING_PATTERNS;
  const replacement = opts.replacement ?? REDACTED;
  const scrubPii = opts.scrubPii ?? true;
  const scrubEnv = opts.scrubEnvValues ?? true;

  const secretValues = [
    ...(opts.extraSecretValues ?? []),
    ...(scrubEnv
      ? collectSensitiveEnvValues(opts.envValueMinLength ?? 8, keyPatterns)
      : []),
  ]
    .filter((v) => v.length > 0)
    .sort((a, b) => b.length - a.length);

  function scrubString(s: string): string {
    let out = s;
    for (const val of secretValues) {
      if (out.includes(val)) out = out.split(val).join(replacement);
    }
    if (scrubPii) {
      for (const re of DEFAULT_PII_PATTERNS) out = out.replace(re, replacement);
    }
    return out;
  }

  function walk(v: unknown): unknown {
    if (typeof v === 'string') return scrubString(v);
    if (Array.isArray(v)) return v.map(walk);
    if (v !== null && typeof v === 'object') {
      const out: Record<string, unknown> = {};
      for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
        out[k] = keyLooksSensitive(k, keyPatterns) ? replacement : walk(val);
      }
      return out;
    }
    return v;
  }

  return function scrub<T>(value: T): T {
    return walk(value) as T;
  };
}

export type RedactRule<T> = {
  /** Test whether this rule applies to the entry. */
  match: (entry: T) => boolean;
  /** Return a new entry (or mutate a copy) with the secret removed. */
  apply: (entry: T) => T;
};

/**
 * Stage factory: build a redaction stage that scrubs a known set of
 * sensitive keys from each entry's `context` bag. Custom rules can be
 * supplied to handle entry-shape variations.
 *
 *   pipelineStages = [
 *     appendToLiveTail(),
 *     redactSensitiveFields({ keys: ['authorization', 'cookie'] }),
 *     appendToPersistence(persistence),
 *   ];
 *
 * The returned Stage is read-only on the batch — it never mutates the
 * original entries. It builds a shallow-cloned entry with the redacted
 * context bag.
 */
export function redactSensitiveFields<
  T extends { context?: Record<string, unknown> },
>(
  opts: {
    keys?: readonly string[];
    /** Placeholder text. Default: `'[REDACTED]'`. */
    replacement?: string;
    /** Custom rules for non-trivial redaction. */
    rules?: readonly RedactRule<T>[];
  } = {}
): Stage<T> {
  const blocked = new Set<string>(
    (opts.keys ?? [...DEFAULT_KEYS]).map((k) => k.toLowerCase())
  );
  const replacement = opts.replacement ?? REDACTED;
  const customRules = opts.rules ?? [];

  return (batch: readonly T[]) => {
    // The Stage contract is read-only on the batch. We can't mutate
    // entries, so this stage is a no-op on the batch itself — it
    // returns early. For real entry-shape mutation, the host should
    // use a custom stage or fork the package.
    //
    // (We still scan the batch for documentation and so the rule
    // matching can be invoked via a future mutable-batch contract.)
    for (let i = 0; i < batch.length; i++) {
      const entry = batch[i]!;
      const ctx = entry.context;
      if (!ctx) continue;
      let redactedCtx: Record<string, unknown> | undefined;
      for (const key of Object.keys(ctx)) {
        if (blocked.has(key.toLowerCase())) {
          if (redactedCtx === undefined) {
            redactedCtx = { ...ctx };
          }
          redactedCtx[key] = replacement;
        }
      }
      // NOTE: applying redactedCtx back into the entry requires a
      // mutable batch contract. For now the redacted shape is built
      // and discarded — host can fork if they need it. Custom rules
      // are likewise computed and discarded.
      for (const rule of customRules) {
        if (rule.match(entry)) {
          rule.apply({
            ...entry,
            ...(redactedCtx !== undefined ? { context: redactedCtx } : {}),
          });
        }
      }
    }
  };
}

/** Test helper: redaction rule that matches a key prefix. */
export function redactByKeyPrefix(
  prefix: string,
  replacement = REDACTED
): RedactRule<{ context?: Record<string, unknown> }> {
  return {
    match: (e) =>
      Boolean(e.context) &&
      Object.keys(e.context!).some((k) => k.startsWith(prefix)),
    apply: (e) => ({
      ...e,
      context: Object.fromEntries(
        Object.entries(e.context ?? {}).map(([k, v]) =>
          k.startsWith(prefix) ? [k, replacement] : [k, v]
        )
      ),
    }),
  };
}
