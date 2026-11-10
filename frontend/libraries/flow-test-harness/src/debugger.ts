import { appendFileSync, readFileSync } from 'node:fs';
import { Session } from 'node:inspector';
import { SourceMap } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, inject } from 'vitest';

declare module 'vitest' {
  // eslint-disable-next-line @typescript-eslint/consistent-type-definitions -- augmentation merges only into an interface
  export interface ProvidedContext {
    flowAppRoot?: string;
  }
}

export type Frame = { function: string; file: string; line: number };
export type Stop = {
  kind: 'breakpoint' | 'logpoint';
  file: string;
  line: number;
  function: string;
  values: Record<string, unknown>;
  stack: Frame[];
  test?: string;
  testFile?: string;
};
export type DebugProbe = { stops: Stop[]; release: () => void };
export type DebugOptions = {
  /** Directory `file` and stack frames are relative to; defaults to the vitest project root. */
  root?: string;
};

type CallFrame = {
  callFrameId: string;
  functionName: string;
  url: string;
  location: { scriptId: string; lineNumber: number; columnNumber: number };
};
type Paused = { callFrames: CallFrame[] };
type Evaluated = {
  result: { value?: unknown; description?: string };
  exceptionDetails?: unknown;
};

function projectRoot(): string {
  try {
    return inject('flowAppRoot') ?? process.cwd();
  } catch {
    return process.cwd();
  }
}

const post = <T>(session: Session, method: string, params: object = {}): T => {
  let out: { err: Error | null; res?: T } | undefined;
  session.post(method, params, (err, res) => {
    out = { err, res: res as T };
  });
  if (!out) throw new Error(`${method}: no synchronous answer`);
  if (out.err) throw out.err;
  return out.res as T;
};

const scriptPath = (url: string) =>
  url.startsWith('file://')
    ? fileURLToPath(url.split('?')[0])
    : url.split('?')[0];

// Frames report positions in the transformed module; its inline source map gives the original line.
function frameOf(
  session: Session,
  root: string,
  maps: Map<string, SourceMap | null>,
  urls: Map<string, string>,
  f: CallFrame
): Frame {
  const url = f.url || urls.get(f.location.scriptId) || '';
  const absolute = scriptPath(url);
  const local = path.isAbsolute(absolute);
  const file = !url ? '(native)' : local ? path.relative(root, absolute) : url;
  const name = f.functionName || '(anonymous)';
  if (!local || file.startsWith('..') || file.includes('node_modules'))
    return { function: name, file, line: f.location.lineNumber + 1 };
  if (!maps.has(f.location.scriptId)) {
    const { scriptSource } = post<{ scriptSource: string }>(
      session,
      'Debugger.getScriptSource',
      { scriptId: f.location.scriptId }
    );
    const inline =
      /sourceMappingURL=data:application\/json[^,]*;base64,([A-Za-z0-9+/=]+)/.exec(
        scriptSource
      )?.[1];
    maps.set(
      f.location.scriptId,
      inline
        ? new SourceMap(
            JSON.parse(Buffer.from(inline, 'base64').toString('utf8'))
          )
        : null
    );
  }
  const entry = maps
    .get(f.location.scriptId)
    ?.findEntry(f.location.lineNumber, f.location.columnNumber);
  const original =
    entry && 'originalLine' in entry
      ? entry.originalLine + 1
      : f.location.lineNumber + 1;
  return { function: name, file, line: original };
}

// The test's file stays relative to the project root whatever `root` the probe uses, so the run report can match it.
function currentTest(): Pick<Stop, 'test' | 'testFile'> {
  const { currentTestName, testPath } = expect.getState();
  return {
    test: currentTestName,
    testFile: testPath ? path.relative(projectRoot(), testPath) : undefined,
  };
}

function readValues(
  session: Session,
  top: CallFrame,
  read: string[]
): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const expression of read) {
    const r = post<Evaluated>(session, 'Debugger.evaluateOnCallFrame', {
      callFrameId: top.callFrameId,
      expression,
      returnByValue: true,
    });
    values[expression] = r.exceptionDetails
      ? { error: r.result.description }
      : r.result.value;
  }
  return values;
}

// The first transformed line whose code maps to the original line: the transform reflows statements, so text alone misses them.
function generatedLine(scriptSource: string, lines: string[], line: number): number | undefined {
  const inline = /sourceMappingURL=data:application\/json[^,]*;base64,([A-Za-z0-9+/=]+)/.exec(scriptSource)?.[1];
  if (!inline) return undefined;
  const map = new SourceMap(JSON.parse(Buffer.from(inline, 'base64').toString('utf8')));
  for (let l = 0; l < lines.length; l++) {
    const column = lines[l].search(/\S/);
    if (column < 0) continue;
    const entry = map.findEntry(l, column);
    if (entry && 'originalLine' in entry && entry.originalLine === line - 1) return l;
  }
  return undefined;
}

function placeBreakpoint(
  session: Session,
  scriptId: string,
  wanted: string,
  line: number
): string {
  const { scriptSource } = post<{ scriptSource: string }>(
    session,
    'Debugger.getScriptSource',
    { scriptId }
  );
  const lines = scriptSource.split('\n');
  const byMap = generatedLine(scriptSource, lines, line);
  const byText = lines.findIndex((l) => l.trim() === wanted);
  const location = { scriptId, lineNumber: byMap ?? (byText >= 0 ? byText : line - 1) };
  return post<{ breakpointId: string }>(session, 'Debugger.setBreakpoint', {
    location,
  }).breakpointId;
}

type Probe = {
  kind: Stop['kind'];
  file: string;
  line: number;
  read: string[];
  root: string;
};

function recordStops(
  session: Session,
  p: Probe,
  scripts: { scriptId: string; url: string }[]
): Stop[] {
  const stops: Stop[] = [];
  const maps = new Map<string, SourceMap | null>();
  session.on('Debugger.paused', ({ params }) => {
    const frames = (params as Paused).callFrames;
    const urls = new Map(scripts.map((x) => [x.scriptId, x.url]));
    const stop: Stop = {
      kind: p.kind,
      file: p.file,
      line: p.line,
      function: frames[0].functionName,
      values: readValues(session, frames[0], p.read),
      stack: frames.map((f) => frameOf(session, p.root, maps, urls, f)),
      ...currentTest(),
    };
    stops.push(stop);
    if (process.env.FLOW_DEBUG_OUT)
      appendFileSync(process.env.FLOW_DEBUG_OUT, `${JSON.stringify(stop)}\n`);
    post(session, 'Debugger.resume');
  });
  return stops;
}

function probe(
  kind: Stop['kind'],
  file: string,
  line: number,
  read: string[],
  options: DebugOptions
): DebugProbe {
  const name = kind === 'breakpoint' ? 'breakAt' : 'logAt';
  const root = options.root ? path.resolve(options.root) : projectRoot();
  const absolute = path.resolve(root, file);
  const wanted = readFileSync(absolute, 'utf8').split('\n')[line - 1]?.trim();
  if (!wanted)
    throw new Error(`${name}: ${file}:${line} is empty or past the end`);
  const session = new Session();
  session.connect();
  const scripts: { scriptId: string; url: string }[] = [];
  session.on('Debugger.scriptParsed', ({ params }) => scripts.push(params));
  const stops = recordStops(session, { kind, file, line, read, root }, scripts);
  post(session, 'Debugger.enable');
  const script = scripts.filter((s) => scriptPath(s.url) === absolute).pop();
  if (!script) session.disconnect();
  if (!script)
    throw new Error(
      `${name}: ${file} is not loaded in this test (import it first)`
    );
  const breakpointId = placeBreakpoint(session, script.scriptId, wanted, line);
  const release = () => {
    post(session, 'Debugger.removeBreakpoint', { breakpointId });
    post(session, 'Debugger.disable');
    session.disconnect();
  };
  return { stops, release };
}

/**
 * Stops the test's own thread at `file:line` (relative to the app root) through the node inspector, reads `read` in
 * the paused frame with the stack at original lines, and resumes. With FLOW_DEBUG_OUT set, each stop is a JSON line.
 */
export function breakAt(
  file: string,
  line: number,
  read: string[],
  options: DebugOptions = {}
): DebugProbe {
  return probe('breakpoint', file, line, read, options);
}

/** A logpoint: records `read` and the stack each time `file:line` runs, and the code carries on. */
export function logAt(
  file: string,
  line: number,
  read: string[],
  options: DebugOptions = {}
): DebugProbe {
  return probe('logpoint', file, line, read, options);
}
