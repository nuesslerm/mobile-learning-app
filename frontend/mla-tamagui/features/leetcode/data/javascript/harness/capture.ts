import { UNPRINTABLE } from './values';

export const OUTPUT_CAP = 256 * 1024;
export const TRUNCATION_MARK = '\n...[output truncated]';

export type Stream = 'stdout' | 'stderr';

/** What one case wrote: each stream is cut at `cap` with a marker, and what is written after the cut is swallowed. */
export type Capture = {
  readonly stdout: string;
  readonly stderr: string;
  write(stream: Stream, text: string): void;
};

export function createCapture(cap: number = OUTPUT_CAP): Capture {
  const text = { stdout: '', stderr: '' };
  const size = { stdout: 0, stderr: 0 };
  return {
    get stdout() {
      return text.stdout;
    },
    get stderr() {
      return text.stderr;
    },
    write(stream, chunk) {
      if (size[stream] >= cap) return;
      size[stream] += chunk.length;
      text[stream] += size[stream] > cap ? chunk.slice(0, chunk.length - (size[stream] - cap)) + TRUNCATION_MARK : chunk;
    },
  };
}

function circularSafe() {
  const ancestors: unknown[] = [];
  return function (this: unknown, _key: string, value: unknown): unknown {
    if (typeof value === 'bigint') return value.toString();
    if (typeof value !== 'object' || value === null) return value;
    while (ancestors.length > 0 && ancestors[ancestors.length - 1] !== this) ancestors.pop();
    if (ancestors.includes(value)) return '[Circular]';
    ancestors.push(value);
    return value;
  };
}

function show(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value === undefined) return 'undefined';
  if (typeof value === 'number') return Object.is(value, -0) ? '-0' : String(value);
  if (typeof value === 'function' || typeof value === 'symbol' || typeof value === 'bigint') return String(value);
  try {
    return JSON.stringify(value, circularSafe()) ?? String(value);
  } catch {
    return UNPRINTABLE;
  }
}

function substitute(template: string, rest: unknown[]): { text: string; used: number } {
  let used = 0;
  const text = template.replace(/%[sdifjoOc%]/g, (token) => {
    if (token === '%%') return '%';
    if (used >= rest.length) return token;
    const arg = rest[used++];
    switch (token) {
      case '%s':
        return typeof arg === 'object' && arg !== null ? show(arg) : String(arg);
      case '%d':
        return typeof arg === 'object' && arg !== null ? 'NaN' : String(Number(arg));
      case '%i':
        return String(Number.parseInt(String(arg), 10));
      case '%f':
        return String(Number.parseFloat(String(arg)));
      case '%c':
        return '';
      default:
        return show(arg);
    }
  });
  return { text, used };
}

/** console.log's text: a string as it is, any other value as compact JSON ([Circular] for a cycle), and %s %d %i %f %j %o %O substituted. */
export function formatLog(args: unknown[]): string {
  const [first, ...rest] = args;
  if (typeof first === 'string' && first.includes('%')) {
    const { text, used } = substitute(first, rest);
    return [text, ...rest.slice(used).map(show)].join(' ');
  }
  return args.map(show).join(' ');
}

type Sink = () => Capture;
const noop = (): void => undefined;

/** A console whose log, info and debug write to the current case's stdout and whose warn and error write to its stderr; every other method does nothing. */
export function createConsole(sink: Sink): Record<string, unknown> {
  const line = (stream: Stream) => (...args: unknown[]) => sink().write(stream, `${formatLog(args)}\n`);
  const methods: Record<string, (...args: unknown[]) => void> = {
    log: line('stdout'),
    info: line('stdout'),
    debug: line('stdout'),
    warn: line('stderr'),
    error: line('stderr'),
    trace: line('stderr'),
    dir: line('stdout'),
    table: line('stdout'),
  };
  return new Proxy(methods, { get: (target, key) => (typeof key === 'string' ? (target[key] ?? noop) : undefined) });
}

/** The one piece of Node's process a solution is likely to use: process.stdout.write and process.stderr.write. */
export function createProcess(sink: Sink): { stdout: { write(text: unknown): boolean }; stderr: { write(text: unknown): boolean } } {
  const stream = (name: Stream) => ({
    write(text: unknown): boolean {
      sink().write(name, typeof text === 'string' ? text : show(text));
      return true;
    },
  });
  return { stdout: stream('stdout'), stderr: stream('stderr') };
}
