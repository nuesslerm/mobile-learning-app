// THE logging API for any host: `Logger` from `@lib/logging`, never `console.*`
// (ESLint `no-console`). Transports are the host's, wired once via
// `configureLogger()`.
//
//   Logger.info('route.change', { to: '/home' });
//   Logger.withError(err).error('api.fail', { endpoint });
//
// Nothing about where a line comes from is passed by hand: the
// `@lib/logging/babel` plugin rewrites every `Logger.<method>(...)` call to
// `Logger.withContext({ file, line, fn, fnPath, testID }).<method>(...)` at
// compile time (src/babel/injectLoggerSource.ts).

import type { ILogLayer } from 'loglayer';

import { scrubberPlugin } from '../plugins/scrubberPlugin.js';
import type { LoggerConfig, LogChain, WideEventFields } from '../types.js';

let config: LoggerConfig | null = null;

/**
 * Wire the static `Logger` to the host's root `LogLayer` and event sink.
 *
 * Call this ONCE at boot (typically in the root `_layout.tsx`), after the
 * host's `LogLayer` is fully constructed. Plugins (sprintf, filter) should
 * already be installed on the root layer BEFORE `configureLogger()` runs.
 */
export function configureLogger(c: LoggerConfig): void {
  config = c;
  // Attach the secret + PII scrubber to the root layer by default, so every log
  // (all transports) is scrubbed before it leaves. Opt out with `scrub: false`.
  // `addPlugins` lives on the concrete LogLayer, not the ILogLayer interface —
  // feature-detect so a null root or a mock without it is simply skipped.
  if (c.scrub !== false) {
    const layer = c.root() as { addPlugins?: (plugins: unknown[]) => void } | null;
    if (layer && typeof layer.addPlugins === 'function') {
      layer.addPlugins([scrubberPlugin(typeof c.scrub === 'object' ? c.scrub : undefined)]);
    }
  }
}

/** Returns true once `configureLogger()` has been called. */
export function isLoggerConfigured(): boolean {
  return config !== null;
}

/** Test-only: forget the wired config. */
export function __resetLoggerConfig(): void {
  config = null;
}

function root(): ILogLayer | null {
  return config?.root() ?? null;
}

// --- core class -----------------------------------------------------------

/** Static methods for a one-off line; `with*` returns a chainable instance. */
export class Logger {
  private static emit(
    level: 'info' | 'debug' | 'warn' | 'error',
    message: string,
    metadata?: WideEventFields
  ): void {
    const layer = root();
    if (!layer) return;
    const hasMeta = metadata && Object.keys(metadata).length > 0;

    let chain: LogChain = layer;
    if (hasMeta) chain = chain.withMetadata(normalizeErrors(metadata ?? {}));
    chain[level](message);
  }

  // --- static ceremony-free API --------------------------------------------

  static info(message: string, metadata?: WideEventFields): void {
    Logger.emit('info', message, metadata);
  }

  static debug(message: string, metadata?: WideEventFields): void {
    Logger.emit('debug', message, metadata);
  }

  static warn(message: string, metadata?: WideEventFields): void {
    Logger.emit('warn', message, metadata);
  }

  static error(message: string, metadata?: WideEventFields): void {
    Logger.emit('error', message, metadata);
  }

  static withMetadata(metadata: WideEventFields): Logger {
    return new Logger({ metadata });
  }

  static withContext(context: WideEventFields): Logger {
    return new Logger({ context });
  }

  static withError(err: unknown): Logger {
    return new Logger({ error: err });
  }

  // --- instance API --------------------------------------------------------

  // Internal field for an attached Error instance — `boundError` to avoid
  // colliding with the public `.error()` log method name (TS strict mode
  // chokes on the name clash even though they're in different namespaces).
  private readonly context: WideEventFields;
  private readonly metadata: WideEventFields;
  private readonly boundError?: unknown;

  constructor(opts?: {
    context?: WideEventFields;
    metadata?: WideEventFields;
    error?: unknown;
  }) {
    this.context = opts?.context ?? {};
    this.metadata = opts?.metadata ?? {};
    this.boundError = opts?.error;
  }

  withContext(context: WideEventFields): Logger {
    return new Logger({
      context: { ...this.context, ...context },
      metadata: this.metadata,
      error: this.boundError,
    });
  }

  withMetadata(metadata: WideEventFields): Logger {
    return new Logger({
      context: this.context,
      metadata: { ...this.metadata, ...metadata },
      error: this.boundError,
    });
  }

  withError(err: unknown): Logger {
    return new Logger({
      context: this.context,
      metadata: this.metadata,
      error: err,
    });
  }

  private emit(
    level: 'info' | 'debug' | 'warn' | 'error',
    message: string,
    extraMeta?: WideEventFields
  ): void {
    const layer = root();
    if (!layer) return;
    let chain: LogChain = layer;

    if (Object.keys(this.context).length > 0) {
      chain = (chain as ILogLayer).withContext(this.context);
    }
    if (this.boundError !== undefined) {
      chain = (chain as ILogLayer).withError(this.boundError);
    }
    const merged: WideEventFields = normalizeErrors({
      ...this.metadata,
      ...(extraMeta ?? {}),
    });
    if (Object.keys(merged).length > 0) {
      chain = chain.withMetadata(merged);
    }
    chain[level](message);
  }

  info(message: string, metadata?: WideEventFields): void {
    this.emit('info', message, metadata);
  }

  debug(message: string, metadata?: WideEventFields): void {
    this.emit('debug', message, metadata);
  }

  warn(message: string, metadata?: WideEventFields): void {
    this.emit('warn', message, metadata);
  }

  error(message: string, metadata?: WideEventFields): void {
    this.emit('error', message, metadata);
  }
}

/**
 * Replaces `Error` values in metadata with a JSON-serializable shape.
 *
 * `Error`'s own `message`/`stack`/`name` are NON-ENUMERABLE, so an Error handed
 * to a transport that does `JSON.stringify` serializes as `{}` — the event
 * arrives, the reason it was logged does not. This repo's documented convention
 * for catch blocks is `withMetadata({ err })`, and every such call site was
 * quietly shipping an empty object, including to BetterStack. Normalising here
 * fixes all of them at once rather than asking each to remember.
 *
 * Only top-level values are converted, which is where the convention puts them.
 */
function normalizeErrors(fields: WideEventFields): WideEventFields {
  let hasError = false;
  for (const value of Object.values(fields)) {
    if (value instanceof Error) {
      hasError = true;
      break;
    }
  }
  if (!hasError) return fields;

  const out: WideEventFields = {};
  for (const [key, value] of Object.entries(fields)) {
    out[key] =
      value instanceof Error
        ? { name: value.name, message: value.message, ...(value.stack ? { stack: value.stack } : {}) }
        : value;
  }
  return out;
}
