import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import type { Reporter, TestCase, TestModule, Vitest } from 'vitest/node';

import type { Stop } from '../debugger';

import {
  renderReportHtml,
  type FlowGolden,
  type FlowReport,
  type FlowReportError,
  type FlowReportFrame,
  type FlowTestReport,
} from './report';

type RunEnd = NonNullable<Reporter['onTestRunEnd']>;
type ConsoleLog = Parameters<NonNullable<Reporter['onUserConsoleLog']>>[0];
type RawError = { message?: string; stack?: string };

export type FlowReporterOptions = {
  /** The package under test; every path in the report is relative to it. */
  root: string;
  /** Where report.json and index.html go; default `<root>/.flow-report`. */
  dir?: string;
  /** Committed golden PNGs (`<suite>/<step>.<scheme>.png`); default `<root>/tests/goldens`. */
  goldensDir?: string;
  /** The JSON-lines file breakAt/logAt append to. */
  debugOut: string;
  /** Truncate `debugOut` when a run starts (true when the harness chose the path). */
  clearDebug: boolean;
};

const MAX_WARNINGS = 50;
const MAX_WARNING_CHARS = 4000;
const SLOWEST = 5;
const FRAME_RE =
  /^\s*at (?:(.+?) \()?((?:file:\/\/)?[^()\s]+?):(\d+):(\d+)\)?$/gm;

function summarize(
  tests: FlowTestReport[],
  files: number
): FlowReport['summary'] {
  const count = (v: FlowTestReport['verdict']) =>
    tests.filter((t) => t.verdict === v).length;
  return {
    total: tests.length,
    passed: count('passed'),
    failed: count('failed'),
    skipped: count('skipped') + count('pending'),
    files,
    slowest: [...tests]
      .sort((a, b) => b.durationMs - a.durationMs)
      .slice(0, SLOWEST)
      .map(({ file, name, durationMs }) => ({ file, name, durationMs })),
  };
}

export class FlowReporter implements Reporter {
  private ctx: Vitest | undefined;
  private started = Date.now();
  private readonly logs = new Map<string, string[]>();
  private runWarnings: string[] = [];
  private readonly dir: string;
  private readonly goldensDir: string;

  constructor(private readonly options: FlowReporterOptions) {
    this.dir = options.dir ?? path.join(options.root, '.flow-report');
    this.goldensDir =
      options.goldensDir ?? path.join(options.root, 'tests', 'goldens');
  }

  onInit(ctx: Vitest): void {
    this.ctx = ctx;
    this.begin();
  }

  onTestRunStart(): void {
    this.begin();
  }

  onUserConsoleLog(log: ConsoleLog): void {
    if (log.type !== 'stderr') return;
    const text = log.content.trimEnd().slice(0, MAX_WARNING_CHARS);
    const list = log.taskId
      ? (this.logs.get(log.taskId) ?? [])
      : this.runWarnings;
    if (list.length < MAX_WARNINGS) list.push(text);
    if (log.taskId) this.logs.set(log.taskId, list);
  }

  async onTestRunEnd(
    ...[modules, unhandled, reason]: Parameters<RunEnd>
  ): Promise<void> {
    const htmlPath = path.join(this.dir, 'index.html');
    try {
      const report = this.build(modules, unhandled, reason);
      const jsonPath = path.join(this.dir, 'report.json');
      mkdirSync(this.dir, { recursive: true });
      writeFileSync(jsonPath, `${JSON.stringify(report, null, 2)}\n`);
      writeFileSync(htmlPath, renderReportHtml(report));
      // Reporters end concurrently; one tick lets the default summary print first, so this is the last line.
      await new Promise((resolve) => setImmediate(resolve));
      this.ctx?.logger.log(`flow report: ${htmlPath}`);
    } catch (error) {
      this.ctx?.logger.error(
        `flow report: failed to write ${htmlPath}: ${(error as Error).stack ?? String(error)}`
      );
    }
  }

  private begin(): void {
    this.started = Date.now();
    this.logs.clear();
    this.runWarnings = [];
    mkdirSync(this.dir, { recursive: true });
    if (this.options.clearDebug) {
      mkdirSync(path.dirname(this.options.debugOut), { recursive: true });
      writeFileSync(this.options.debugOut, '');
    }
  }

  private rel(file: string): string {
    return path.relative(this.options.root, file);
  }

  // Vitest leaves `stacks` unset on results, but the worker already source-mapped the `stack` string.
  private frames(stack: string | undefined): FlowReportFrame[] {
    return [...(stack ?? '').matchAll(FRAME_RE)]
      .map(([, method = '', url, line, column]) => ({
        method,
        file: url.replace(/^file:\/\//, ''),
        line: Number(line),
        column: Number(column),
      }))
      .filter(
        (f) => path.isAbsolute(f.file) && !f.file.includes('node_modules')
      )
      .map((f) => ({ ...f, file: this.rel(f.file) }));
  }

  private error(raw: RawError, file?: string): FlowReportError {
    return {
      message: raw.message ?? String(raw),
      stack: raw.stack,
      frames: this.frames(raw.stack),
      ...(file ? { file } : {}),
    };
  }

  private stops(): Stop[] {
    if (!existsSync(this.options.debugOut)) return [];
    return readFileSync(this.options.debugOut, 'utf8')
      .split('\n')
      .filter(Boolean)
      .flatMap((line) => {
        try {
          return [JSON.parse(line) as Stop];
        } catch {
          return [];
        }
      });
  }

  // The snapshot is `<out>/<suite>/<step>.html`; `wf flow goldens` writes its PNGs under `<out>/../out/<suite>/`.
  private golden(snapshot: string): FlowGolden {
    const scheme = process.env.FLOW_THEME ?? 'light';
    const step = path.basename(snapshot, '.html');
    const suite = path.basename(path.dirname(snapshot));
    const runDir = path.dirname(path.dirname(path.dirname(snapshot)));
    const stem = path.join(runDir, 'out', suite, `${step}.${scheme}`);
    const golden = path.join(this.goldensDir, suite, `${step}.${scheme}.png`);
    return {
      step: `${suite}/${step}`,
      snapshot: this.rel(snapshot),
      golden: this.rel(golden),
      goldenExists: existsSync(golden),
      actual: this.rel(`${stem}.actual.png`),
      expected: this.rel(`${stem}.expected.png`),
      diff: this.rel(`${stem}.diff.png`),
    };
  }

  private test(tc: TestCase, file: string, stops: Stop[]): FlowTestReport {
    const result = tc.result();
    return {
      file,
      name: tc.fullName,
      verdict: result.state,
      durationMs: tc.diagnostic()?.duration ?? 0,
      errors: (result.errors ?? []).map((e) => this.error(e)),
      goldens: (tc.meta().flowSnapshots ?? []).map((s) => this.golden(s)),
      stops: stops.filter((s) => s.testFile === file && s.test === tc.fullName),
      warnings: this.logs.get(tc.id) ?? [],
    };
  }

  private build(
    modules: ReadonlyArray<TestModule>,
    unhandled: Parameters<RunEnd>[1],
    reason: Parameters<RunEnd>[2]
  ): FlowReport {
    const root = this.options.root;
    const stops = this.stops();
    const tests: FlowTestReport[] = [];
    const errors: FlowReportError[] = [];
    for (const module of modules) {
      const file = this.rel(module.moduleId);
      for (const e of module.errors()) errors.push(this.error(e, file));
      for (const tc of module.children.allTests())
        tests.push(this.test(tc, file, stops));
    }
    for (const e of unhandled) errors.push(this.error(e));
    const finished = Date.now();
    return {
      schema: 'flow-report/1',
      harness: 'flow-test-harness',
      verdict: reason,
      startedAt: new Date(this.started).toISOString(),
      finishedAt: new Date(finished).toISOString(),
      durationMs: finished - this.started,
      root,
      paths: {
        json: this.rel(path.join(this.dir, 'report.json')),
        html: this.rel(path.join(this.dir, 'index.html')),
        debug: this.rel(this.options.debugOut),
      },
      env: {
        goldens: process.env.FLOW_GOLDENS === '1',
        theme: process.env.FLOW_THEME ?? null,
      },
      summary: summarize(tests, modules.length),
      errors,
      warnings: this.runWarnings,
      tests,
    };
  }
}
