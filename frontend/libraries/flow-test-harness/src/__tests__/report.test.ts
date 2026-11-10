import { describe, expect, it } from 'vitest';

import {
  renderReportHtml,
  type FlowReport,
  type FlowTestReport,
} from '../vitest/report';

const ROOT = '/repo/app';

function testReport(overrides: Partial<FlowTestReport>): FlowTestReport {
  return {
    file: 'tests/flow/a.flow.test.tsx',
    name: 'suite > Given a, When b, Then c',
    verdict: 'passed',
    durationMs: 120,
    errors: [],
    goldens: [],
    stops: [],
    warnings: [],
    ...overrides,
  };
}

function report(tests: FlowTestReport[]): FlowReport {
  const failed = tests.filter((t) => t.verdict === 'failed').length;
  return {
    schema: 'flow-report/1',
    harness: 'flow-test-harness',
    verdict: failed ? 'failed' : 'passed',
    startedAt: '2026-10-14T10:00:00.000Z',
    finishedAt: '2026-10-14T10:00:05.000Z',
    durationMs: 5000,
    root: ROOT,
    paths: {
      json: '.flow-report/report.json',
      html: '.flow-report/index.html',
      debug: '.flow-report/debug.jsonl',
    },
    env: { goldens: false, theme: null },
    summary: {
      total: tests.length,
      passed: tests.length - failed,
      failed,
      skipped: 0,
      files: 1,
      slowest: tests.map(({ file, name, durationMs }) => ({
        file,
        name,
        durationMs,
      })),
    },
    errors: [],
    warnings: [],
    tests,
  };
}

const parse = (html: string): Document =>
  new DOMParser().parseFromString(html, 'text/html');

describe('flow report HTML', () => {
  it('Given a test name and a stop value carrying markup, When the report renders, Then both show as text and no script element exists', () => {
    // Given a test whose name and debugger values hold HTML
    const html = renderReportHtml(
      report([
        testReport({
          name: 'suite > Given <script>alert(1)</script> & "quotes"',
          stops: [
            {
              kind: 'logpoint',
              file: 'lib/a.ts',
              line: 3,
              function: 'f',
              values: { label: '<img src=x onerror=alert(1)>' },
              stack: [{ function: 'f', file: 'lib/a.ts', line: 3 }],
            },
          ],
        }),
      ])
    );

    // When it is parsed as a page
    const doc = parse(html);

    // Then the markup is text, not elements
    expect(doc.querySelectorAll('script')).toHaveLength(0);
    expect(doc.querySelectorAll('main img')).toHaveLength(0);
    expect(doc.querySelector('.test .name')?.textContent).toBe(
      'suite > Given <script>alert(1)</script> & "quotes"'
    );
    expect(doc.querySelector('.stop pre')?.textContent).toContain(
      '<img src=x onerror=alert(1)>'
    );
  });

  it('Given one passing and one failing test, When the report renders, Then the verdict is failed, the failure is open first and shows its message and location', () => {
    // Given a run with a failure
    const failing = testReport({
      file: 'tests/flow/b.flow.test.tsx',
      name: 'suite > Given x, When y, Then z',
      verdict: 'failed',
      errors: [
        {
          message: 'expected 2 to be 3',
          stack:
            'AssertionError: expected 2 to be 3\n    at tests/flow/b.flow.test.tsx:12:5',
          frames: [
            {
              file: 'tests/flow/b.flow.test.tsx',
              line: 12,
              column: 5,
              method: '',
            },
          ],
        },
      ],
    });

    // When the report renders
    const doc = parse(renderReportHtml(report([testReport({}), failing])));

    // Then the header says failed and the failing test leads, open, with its error
    expect(doc.querySelector('h1 .pill')?.textContent).toBe('failed');
    const first = doc.querySelector('details.test');
    expect(first?.classList.contains('failed')).toBe(true);
    expect(first?.hasAttribute('open')).toBe(true);
    expect(first?.querySelector('.error .message')?.textContent).toBe(
      'expected 2 to be 3'
    );
    expect(first?.querySelector('.error .where')?.textContent).toBe(
      'at tests/flow/b.flow.test.tsx:12:5'
    );
    expect(doc.querySelector('details.test.passed')?.hasAttribute('open')).toBe(
      false
    );
  });

  it('Given a test that wrote a golden snapshot, When the report renders, Then it links the snapshot, golden, actual, expected and diff relative to the report', () => {
    // Given a final-state golden with a committed PNG
    const golden = {
      step: 'a/final-x-abc123',
      snapshot: '.flow-goldens/light/snapshots/a/final-x-abc123.html',
      golden: 'tests/goldens/a/final-x-abc123.light.png',
      goldenExists: true,
      actual: '.flow-goldens/light/out/a/final-x-abc123.light.actual.png',
      expected: '.flow-goldens/light/out/a/final-x-abc123.light.expected.png',
      diff: '.flow-goldens/light/out/a/final-x-abc123.light.diff.png',
    };

    // When the report renders
    const doc = parse(
      renderReportHtml({
        ...report([testReport({ goldens: [golden] })]),
        env: { goldens: true, theme: 'light' },
      })
    );

    // Then every file is linked from the report's own directory, and the run's screenshot is shown in the test and in the gallery
    const hrefs = Array.from(doc.querySelectorAll('.test .golden a'), (a) => [
      a.textContent,
      a.getAttribute('href'),
    ]);
    expect(hrefs).toEqual([
      [
        'snapshot html',
        '../.flow-goldens/light/snapshots/a/final-x-abc123.html',
      ],
      ['golden png', '../tests/goldens/a/final-x-abc123.light.png'],
      [
        'actual',
        '../.flow-goldens/light/out/a/final-x-abc123.light.actual.png',
      ],
      [
        'expected',
        '../.flow-goldens/light/out/a/final-x-abc123.light.expected.png',
      ],
      ['diff', '../.flow-goldens/light/out/a/final-x-abc123.light.diff.png'],
    ]);
    const shots = Array.from(doc.querySelectorAll('.golden img'), (i) =>
      i.getAttribute('src')
    );
    expect(shots).toEqual([
      '../.flow-goldens/light/out/a/final-x-abc123.light.actual.png',
      '../.flow-goldens/light/out/a/final-x-abc123.light.actual.png',
    ]);
  });

  it('Given a run summary, When the report renders, Then the counts, the slowest tests and a dark scheme are on the page', () => {
    // Given / When
    const html = renderReportHtml(
      report([
        testReport({ durationMs: 2500 }),
        testReport({ name: 'suite > quick', durationMs: 10 }),
      ])
    );
    const doc = parse(html);

    // Then
    const stats = Array.from(
      doc.querySelectorAll('.stat'),
      (s) => s.textContent
    );
    expect(stats).toEqual([
      '2tests',
      '2passed',
      '0failed',
      '0skipped',
      '1files',
      '5.0 stotal time',
    ]);
    expect(doc.querySelector('ol.slowest li')?.textContent).toContain('2.5 s');
    expect(html).toContain('prefers-color-scheme:dark');
    expect(doc.querySelector('meta[name="viewport"]')).not.toBeNull();
  });
});
