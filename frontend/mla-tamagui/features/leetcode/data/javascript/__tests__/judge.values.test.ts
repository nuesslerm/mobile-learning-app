import { describe, expect, it } from 'vitest';

import { canonicalJson, compareValues, toWire } from '../harness/values';

describe('toWire: what a solution returned, as JSON', () => {
  it('Given undefined as the whole answer or inside an array or object, When it is read, Then it is null', () => {
    expect(toWire(undefined)).toBeNull();
    expect(toWire([1, undefined, 3])).toEqual([1, null, 3]);
    expect(toWire({ a: undefined })).toEqual({ a: null });
  });

  it('Given a hole in an array, When it is read, Then it is null', () => {
    const sparse: unknown[] = [1];
    sparse[2] = 3;
    expect(toWire(sparse)).toEqual([1, null, 3]);
  });

  it('Given NaN, Infinity and -Infinity, When they are read, Then each is its own text, alone or nested', () => {
    expect(toWire(Number.NaN)).toBe('NaN');
    expect(toWire(Infinity)).toBe('Infinity');
    expect(toWire(-Infinity)).toBe('-Infinity');
    expect(toWire([1, Number.NaN])).toEqual([1, 'NaN']);
  });

  it('Given a BigInt, When it is read, Then it is a number when it is safe and its digits as a string otherwise', () => {
    expect(toWire(5n)).toBe(5);
    expect(toWire(-9007199254740991n)).toBe(-9007199254740991);
    expect(toWire(2n ** 70n)).toBe('1180591620717411303424');
    expect(toWire(9007199254740992n)).toBe('9007199254740992');
  });

  it('Given a Set, a Map, a function and a class instance, When they are read, Then each is shown by String and none is turned into an array or an object', () => {
    class Pair {
      constructor(readonly a = 1) {}
    }
    expect(toWire(new Set([1, 2]))).toBe('[object Set]');
    expect(toWire(new Map([[1, 2]]))).toBe('[object Map]');
    expect(toWire(function named() {})).toContain('named');
    expect(toWire(new Pair())).toBe('[object Object]');
    expect(toWire(Symbol('s'))).toBe('Symbol(s)');
  });

  it('Given a cyclic value, When it is read, Then it is shown by String instead of failing', () => {
    const loop: unknown[] = [1];
    loop.push(loop);
    expect(toWire(loop)).toBe('1,');
    const node: Record<string, unknown> = { name: 'n' };
    node.self = node;
    expect(toWire(node)).toBe('[object Object]');
  });

  it('Given the same array used twice but no cycle, When it is read, Then both uses are copied', () => {
    const shared = [1];
    expect(toWire([shared, shared])).toEqual([[1], [1]]);
  });

  it('Given a value whose own String throws, When it is read, Then it is shown as <unprintable>', () => {
    class Hostile {
      toString(): string {
        throw new Error('no');
      }
    }
    expect(toWire(new Hostile())).toBe('<unprintable>');
  });

  it('Given -0, When it is read and printed as JSON, Then it is 0', () => {
    expect(JSON.stringify(toWire(-0))).toBe('0');
  });
});

describe('canonicalJson', () => {
  it('Given objects with the same keys in two orders, When they are printed, Then the text is the same', () => {
    expect(canonicalJson({ b: 1, a: { d: 2, c: 3 } })).toBe(canonicalJson({ a: { c: 3, d: 2 }, b: 1 }));
    expect(canonicalJson({ b: 1, a: [2, { z: 1, y: 0 }] })).toBe('{"a":[2,{"y":0,"z":1}],"b":1}');
  });
});

describe('compareValues: exact', () => {
  it.each([
    ['two equal arrays', [1, 2, 3], [1, 2, 3], true],
    ['arrays of the same items in another order', [1, 2, 3], [3, 2, 1], false],
    ['objects whose keys come in another order', { a: 1, b: 2 }, { b: 2, a: 1 }, true],
    ['a number and its digits as a string', 1, '1', false],
    ['true and 1', true, 1, false],
    ['null and undefined', null, undefined, true],
    ['-0 and 0', -0, 0, true],
    ['NaN and a finite number', Number.NaN, 0, false],
    ['NaN and null', Number.NaN, null, false],
    ['Infinity and a huge finite number', Infinity, Number.MAX_VALUE, false],
    ['a Set and an array', new Set([1]), [1], false],
    ['nested arrays', [[1, 2], [3]], [[1, 2], [3]], true],
  ])('Given %s, When they are compared, Then the answer is %s', (_name, actual, expected, answer) => {
    expect(compareValues(actual, expected)).toBe(answer);
    expect(compareValues(actual, expected, 'exact')).toBe(answer);
  });

  it('Given no mode, When two values are compared, Then the mode is exact', () => {
    expect(compareValues([2, 1], [1, 2], undefined)).toBe(false);
    expect(compareValues([2, 1], [1, 2], null)).toBe(false);
  });
});

describe('compareValues: sorted', () => {
  it.each([
    ['the same items in another order', [1, 0], [0, 1], true],
    ['arrays that differ in one item', [1, 2], [1, 3], false],
    ['arrays of different lengths', [1, 1], [1], false],
    ['the same items with a repeat counted', [1, 1, 2], [1, 2, 2], false],
    ['lists of lists in another outer order', [[1, 2], [0, 5]], [[0, 5], [1, 2]], true],
    ['lists of lists whose inner order differs', [[2, 1]], [[1, 2]], false],
    ['two things that are not arrays', 3, 3, true],
    ['an array and a number', [3], 3, false],
  ])('Given %s, When they are compared as sorted, Then the answer is %s', (_name, actual, expected, answer) => {
    expect(compareValues(actual, expected, 'sorted')).toBe(answer);
  });
});

describe('compareValues: setOfSorted', () => {
  it('Given an inner list in another order, When compared, Then it equals', () => {
    expect(compareValues([[2, 1], [3]], [[1, 2], [3]], 'setOfSorted')).toBe(true);
  });

  it('Given the outer order changed, When compared, Then it does not equal', () => {
    expect(compareValues([[3], [1, 2]], [[1, 2], [3]], 'setOfSorted')).toBe(false);
  });

  it('Given numbers of different digit counts, When each inner list is sorted, Then it sorts as numbers and not as text', () => {
    expect(compareValues([[10, 9, 100]], [[9, 10, 100]], 'setOfSorted')).toBe(true);
  });

  it('Given strings, When each inner list is sorted, Then equal sets of strings are equal', () => {
    expect(compareValues([['b', 'a']], [['a', 'b']], 'setOfSorted')).toBe(true);
    expect(compareValues([['b', 'a']], [['a', 'c']], 'setOfSorted')).toBe(false);
  });

  it('Given an item that is not a list, When compared, Then it falls back to exact', () => {
    expect(compareValues([1, 2], [1, 2], 'setOfSorted')).toBe(true);
    expect(compareValues([2, 1], [1, 2], 'setOfSorted')).toBe(false);
  });
});

describe('compareValues: approx', () => {
  it.each([
    ['numbers 0.000009 apart', 1.5, 1.500009, true],
    ['numbers 0.00002 apart', 1.5, 1.50002, false],
    ['arrays whose items are each close', [1, 2.000001], [1.000001, 2], true],
    ['arrays whose lengths differ', [1], [1, 1], false],
    ['true and 1', true, 1, false],
    ['1 and true', 1, true, false],
    ['true and true', true, true, true],
    ['NaN and a finite number', Number.NaN, 0, false],
    ['NaN and NaN', Number.NaN, 1, false],
    ['Infinity and a finite number', Infinity, 1e308, false],
    ['-0 and 0', -0, 0, true],
    ['two equal strings', 'ab', 'ab', true],
    ['a number and its text', 1, '1', false],
  ])('Given %s, When compared as approx, Then the answer is %s', (_name, actual, expected, answer) => {
    expect(compareValues(actual, expected, 'approx')).toBe(answer);
  });
});

describe('compareValues never throws', () => {
  it('Given values that cannot be read, When compared in any mode, Then it answers false', () => {
    const hostile = new Proxy({}, { ownKeys: () => { throw new Error('keys'); }, getPrototypeOf: () => { throw new Error('proto'); } });
    for (const mode of [undefined, 'exact', 'sorted', 'setOfSorted', 'approx']) {
      expect(() => compareValues(hostile, [1], mode)).not.toThrow();
      expect(compareValues(hostile, [1], mode)).toBe(false);
    }
  });
});
