import { describe, expect, it } from 'vitest';

import { describeError, locationFromEvent, locationFromStack } from '../harness/errorLocation';

const URL = 'blob:null/6f1c';
const OFFSET = 200;

const V8_STACK = [
  'TypeError: Cannot read properties of undefined (reading \'x\')',
  `    at twoSum (${URL}:12:19)`,
  `    at Solution.twoSum (${URL}:30:5)`,
  `    at runCase (${URL}:1:150)`,
  `    at ${URL}:1:90`,
].join('\n');

const JSC_STACK = [`twoSum@${URL}:12:19`, `twoSum@${URL}:30:5`, `runCase@${URL}:1:150`, `@${URL}:1:90`, 'global code@'].join('\n');

describe('locationFromStack', () => {
  it('Given a V8 stack, When the script’s own url is given, Then the innermost frame in the script is the location', () => {
    expect(locationFromStack(V8_STACK, URL, OFFSET)).toEqual({ line: 12, column: 19 });
  });

  it('Given a JavaScriptCore stack, When the script’s own url is given, Then the innermost frame in the script is the location', () => {
    expect(locationFromStack(JSC_STACK, URL, OFFSET)).toEqual({ line: 12, column: 19 });
  });

  it('Given harness frames on line 1 before the learner’s text, When the stack is read, Then they are skipped for the learner’s frame', () => {
    const stack = [`Error: boom`, `    at runCase (${URL}:1:150)`, `    at twoSum (${URL}:7:3)`].join('\n');
    expect(locationFromStack(stack, URL, OFFSET)).toEqual({ line: 7, column: 3 });
  });

  it('Given a learner frame on line 1, When the stack is read, Then the prelude’s length is removed from its column', () => {
    const stack = `Error: boom\n    at f (${URL}:1:${OFFSET + 14})\n    at runCase (${URL}:1:150)`;
    expect(locationFromStack(stack, URL, OFFSET)).toEqual({ line: 1, column: 14 });
    expect(locationFromStack(`f@${URL}:1:${OFFSET + 1}`, URL, OFFSET)).toEqual({ line: 1, column: 1 });
  });

  it('Given only harness frames, When the stack is read, Then there is no location', () => {
    expect(locationFromStack(`Error: x\n    at runCase (${URL}:1:150)\n    at ${URL}:1:${OFFSET}`, URL, OFFSET)).toBeNull();
  });

  it('Given a stack with no frame in the script, Then there is no location', () => {
    expect(locationFromStack('Error: x\n    at lib (https://other.test/a.js:3:4)', URL, OFFSET)).toBeNull();
  });

  it('Given a url that is the start of another script’s url, When the stack is read, Then the other script’s frame is not taken', () => {
    expect(locationFromStack('Error: x\n    at f (blob:null/6f1c0:9:9)', URL, OFFSET)).toBeNull();
  });

  it('Given no stack, a thrown string’s missing stack or an empty script name, Then there is no location', () => {
    expect(locationFromStack(undefined, URL, OFFSET)).toBeNull();
    expect(locationFromStack(null, URL, OFFSET)).toBeNull();
    expect(locationFromStack('', URL, OFFSET)).toBeNull();
    expect(locationFromStack(V8_STACK, '', OFFSET)).toBeNull();
  });

  it('Given a frame whose function name holds colons and digits, When the stack is read, Then the position is still the last two numbers', () => {
    expect(locationFromStack(`    at Object.<anonymous>:3:4 (${URL}:21:8)`, URL, OFFSET)).toEqual({ line: 21, column: 8 });
  });
});

describe('locationFromEvent', () => {
  it('Given an ErrorEvent past line 1, When it is read, Then it is its line and column', () => {
    expect(locationFromEvent({ lineno: 4, colno: 9 }, OFFSET)).toEqual({ line: 4, column: 9 });
  });

  it('Given an ErrorEvent on line 1, When it is read, Then the prelude’s length is removed from the column', () => {
    expect(locationFromEvent({ lineno: 1, colno: OFFSET + 6 }, OFFSET)).toEqual({ line: 1, column: 6 });
  });

  it('Given an event with no column, When it is read, Then only the line is given', () => {
    expect(locationFromEvent({ lineno: 3, colno: 0 }, OFFSET)).toEqual({ line: 3 });
    expect(locationFromEvent({ lineno: 3 }, OFFSET)).toEqual({ line: 3 });
  });

  it('Given an event on line 1 inside the prelude, When it is read, Then only the line is given', () => {
    expect(locationFromEvent({ lineno: 1, colno: 10 }, OFFSET)).toEqual({ line: 1 });
  });

  it('Given an event with no line, When it is read, Then there is no location', () => {
    expect(locationFromEvent({}, OFFSET)).toBeNull();
    expect(locationFromEvent({ lineno: 0, colno: 5 }, OFFSET)).toBeNull();
  });
});

describe('describeError', () => {
  it('Given an Error, When it is described, Then it is its name and message', () => {
    expect(describeError(new TypeError('x is not a function'))).toBe('TypeError: x is not a function');
    expect(describeError(new RangeError('Maximum call stack size exceeded'))).toBe('RangeError: Maximum call stack size exceeded');
  });

  it('Given a thrown string, number or null, When it is described, Then it is its text', () => {
    expect(describeError('boom')).toBe('boom');
    expect(describeError(42)).toBe('42');
    expect(describeError(null)).toBe('null');
    expect(describeError(undefined)).toBe('undefined');
  });

  it('Given an error whose own printing throws, When it is described, Then it is <unprintable>', () => {
    class Hostile extends Error {
      override get message(): string {
        throw new Error('no message');
      }
    }
    expect(describeError(new Hostile())).toBe('<unprintable>');
    expect(describeError({ toString: () => { throw new Error('no'); } })).toBe('<unprintable>');
  });
});
