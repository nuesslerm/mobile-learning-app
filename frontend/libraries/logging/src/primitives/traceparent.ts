// W3C `traceparent` propagation for `fetch()` call sites outside a shared
// HTTP client (KTD3) — mobile's `features/*/data`/`features/*/auth` layers
// each wrap their own existing `fetch(url, init)` call with `withTraceparent`
// rather than routing through an intercepting client. Pure and
// platform-neutral (no React/RN import) so it is usable from both mobile
// (pre-U12, always the no-op "no active span" path) and Node hosts alike.
//
// The header is attached ONLY when the destination's origin is in an
// explicit allowlist — trace-correlation data (trace_id/span_id) must never
// leak to a third-party origin that has no legitimate need for it.

/**
 * Structural subset of an OTel `Span` — only what's needed to format a
 * `traceparent` header. Deliberately NOT `@opentelemetry/api`'s `Span` type,
 * so this primitive carries no OTel dependency: a real `Span` already
 * satisfies this shape once a host wires one in.
 */
export type ActiveSpanLike = {
  spanContext(): { traceId: string; spanId: string; traceFlags: number };
};

/** Fetch's `init` argument, narrowed to plain-object headers — every known
 *  call site in this repo builds headers as a `Record<string, string>` (or
 *  omits them), so this avoids taking on `Headers`/tuple-array merge logic
 *  no call site actually needs. */
export type TraceparentFetchInit = Omit<RequestInit, 'headers'> & {
  headers?: Record<string, string>;
};

function originOf(url: string): string | null {
  try {
    // A relative URL (e.g. `/api/code-assist`) has no parseable origin here
    // (no document/window base to resolve against) — treated as "not
    // allowlisted" rather than guessed, so it never gets a header by
    // accident of same-origin assumption.
    return new URL(url).origin;
  } catch {
    return null;
  }
}

/**
 * Derive the allowlisted origins from the app's own configured backend base
 * URLs, rather than a hardcoded hostname list — production values resolve to
 * a Tailscale hostname unknown at build time, and dev/test values are
 * localhost. Malformed or absent config entries are silently skipped (never
 * added), which is the conservative failure direction.
 */
export function allowedTraceparentOrigins(
  baseUrls: readonly (string | null | undefined)[]
): ReadonlySet<string> {
  const origins = new Set<string>();
  for (const url of baseUrls) {
    if (!url) continue;
    const origin = originOf(url);
    if (origin) origins.add(origin);
  }
  return origins;
}

function toHex(n: number, len: number): string {
  return n.toString(16).padStart(len, '0');
}

function formatTraceparent(span: ActiveSpanLike): string {
  const ctx = span.spanContext();
  return `00-${ctx.traceId}-${ctx.spanId}-${toHex(ctx.traceFlags, 2)}`;
}

/**
 * Returns `init` with a W3C `traceparent` header attached, when — and only
 * when — `span` is active AND `url`'s origin is in `allowedOrigins`:
 *   - no active `span` (the pre-U12 default on mobile): returns `init`
 *     unchanged, no header.
 *   - `url` is relative, unparseable, or its origin isn't allowlisted:
 *     returns `init` unchanged, no header — this is the security boundary
 *     that keeps trace-correlation data off third-party destinations.
 */
export function withTraceparent(
  url: string,
  init: TraceparentFetchInit | undefined,
  span: ActiveSpanLike | null | undefined,
  allowedOrigins: ReadonlySet<string>
): TraceparentFetchInit | undefined {
  if (!span) return init;
  const origin = originOf(url);
  if (!origin || !allowedOrigins.has(origin)) return init;
  return {
    ...init,
    headers: { ...init?.headers, traceparent: formatTraceparent(span) },
  };
}
