import { describe, expect, it } from 'vitest';

import { OUTPUT_CAP, TRUNCATION_MARK, createCapture, createConsole, createProcess, formatLog, type Capture } from '../harness/capture';

function surface(cap?: number) {
  let current: Capture = createCapture(cap);
  const consoleLike = createConsole(() => current) as Record<string, (...args: unknown[]) => void>;
  const processLike = createProcess(() => current);
  return { console: consoleLike, process: processLike, get capture() { return current; }, next: () => (current = createCapture(cap)) };
}

describe('where each console method writes', () => {
  it('Given console.log, info and debug, When they are called, Then they go to stdout', () => {
    const s = surface();
    s.console.log!('a');
    s.console.info!('b');
    s.console.debug!('c');
    expect(s.capture.stdout).toBe('a\nb\nc\n');
    expect(s.capture.stderr).toBe('');
  });

  it('Given console.warn and error, When they are called, Then they go to stderr', () => {
    const s = surface();
    s.console.warn!('w');
    s.console.error!('e');
    expect(s.capture.stderr).toBe('w\ne\n');
    expect(s.capture.stdout).toBe('');
  });

  it('Given process.stdout.write and process.stderr.write, When they are called, Then the text goes to the stream with no newline added', () => {
    const s = surface();
    expect(s.process.stdout.write('x')).toBe(true);
    s.process.stdout.write('y');
    s.process.stderr.write('z');
    expect(s.capture.stdout).toBe('xy');
    expect(s.capture.stderr).toBe('z');
  });

  it('Given a console method the harness does not know, When it is called, Then it does nothing and does not throw', () => {
    const s = surface();
    expect(() => s.console.group!('g')).not.toThrow();
    expect(() => s.console.time!('t')).not.toThrow();
    expect(s.capture.stdout).toBe('');
  });

  it('Given two cases, When the second starts, Then its streams are its own', () => {
    const s = surface();
    s.console.log!('first');
    s.next();
    s.console.log!('second');
    expect(s.capture.stdout).toBe('second\n');
  });
});

describe('formatLog', () => {
  it('Given strings, When they are printed, Then they are raw and joined by a space', () => {
    expect(formatLog(['a "b"', 'c'])).toBe('a "b" c');
  });

  it('Given other values, When they are printed, Then each is compact JSON', () => {
    expect(formatLog([1, true, null, [1, 'x'], { a: 1 }])).toBe('1 true null [1,"x"] {"a":1}');
    expect(formatLog([undefined])).toBe('undefined');
    expect(formatLog([-0, 5n])).toBe('-0 5');
  });

  it('Given a cyclic object, When it is printed, Then the cycle is [Circular] and a repeat that is no cycle is printed in full', () => {
    const loop: Record<string, unknown> = { name: 'n' };
    loop.self = loop;
    expect(formatLog([loop])).toBe('{"name":"n","self":"[Circular]"}');
    const shared = [1];
    expect(formatLog([[shared, shared]])).toBe('[[1],[1]]');
  });

  it('Given a format string, When %s %d %j are given, Then each is substituted and the rest follow', () => {
    expect(formatLog(['n=%d s=%s j=%j', 4, 'x', { a: 1 }, 'extra'])).toBe('n=4 s=x j={"a":1} extra');
    expect(formatLog(['100%% sure'])).toBe('100% sure');
    expect(formatLog(['%s and %s', 'only one'])).toBe('only one and %s');
  });

  it('Given a value whose printing throws, When it is printed, Then it is <unprintable>', () => {
    const hostile = { toJSON() { throw new Error('no'); } };
    expect(formatLog([hostile])).toBe('<unprintable>');
  });
});

describe('the output cap', () => {
  it('Given output past the cap, When it is read, Then it is cut at the cap with the marker', () => {
    const capture = createCapture(10);
    capture.write('stdout', '12345678');
    capture.write('stdout', 'abcdef');
    expect(capture.stdout).toBe(`12345678ab${TRUNCATION_MARK}`);
  });

  it('Given a write after the cut, When it is made, Then it is swallowed and the marker is not repeated', () => {
    const capture = createCapture(4);
    capture.write('stdout', 'abcdef');
    capture.write('stdout', 'more');
    expect(capture.stdout).toBe(`abcd${TRUNCATION_MARK}`);
  });

  it('Given exactly the cap, When it is written, Then nothing is cut', () => {
    const capture = createCapture(4);
    capture.write('stdout', 'abcd');
    expect(capture.stdout).toBe('abcd');
  });

  it('Given stdout cut, When stderr is written, Then the two streams are counted apart', () => {
    const capture = createCapture(4);
    capture.write('stdout', 'abcdef');
    capture.write('stderr', 'ok');
    expect(capture.stderr).toBe('ok');
  });

  it('Given 200,000 printed lines at the real cap, When they are read, Then the stream is capped at 256 KiB plus the marker', () => {
    const s = surface();
    for (let i = 0; i < 200_000; i += 1) s.console.log!('line', i);
    expect(s.capture.stdout.length).toBe(OUTPUT_CAP + TRUNCATION_MARK.length);
    expect(s.capture.stdout.endsWith(TRUNCATION_MARK)).toBe(true);
  });
});
