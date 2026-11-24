import type { TestCase } from '../../../domain/run/execution';
import type { Capture } from './capture';
import { describeError } from './errorLocation';
import { DEFAULT_CONSTRUCTORS, ValueError, buildArgs, readValue, reachable, type NodeConstructors } from './nodes';
import { compareValues, toWire } from './values';
import type { CaseRow, HarnessConfig } from '../workerProtocol';

const MAX_STACK = 8192;
const NO_STACK = undefined;

type Method = (this: unknown, ...args: unknown[]) => unknown;
type Constructible = new (...args: unknown[]) => Record<string, unknown>;

/** How the learner's code is called: a method of a class, a function, a class driven by operations, or an encode and decode pair. */
export type Target =
  | { kind: 'method'; call(args: unknown[]): unknown }
  | { kind: 'design'; className: string; construct(args: unknown[]): Record<string, unknown> }
  | { kind: 'roundTrip'; encode(args: unknown[]): unknown; decode(encoded: unknown): unknown; encodeName: string };

const isFunction = (value: unknown): value is Method => typeof value === 'function';
const hasMethod = (value: unknown, name: string): boolean => isFunction(value) && isFunction((value as { prototype?: Record<string, unknown> }).prototype?.[name]);

function callMethod(instance: Record<string, unknown>, name: string, args: unknown[], owner: string): unknown {
  const method = instance[name];
  if (!isFunction(method)) throw new TypeError(`${owner}.${name} is not a function`);
  return method.apply(instance, args);
}

/** Finds what to call by the names of the catalog's starters; the error says what is missing. `lookup` reads a top-level name of the learner's code. */
export function resolveTarget(config: HarnessConfig, lookup: (name: string) => unknown): { target: Target } | { error: string } {
  const { harness, methodName } = config;
  if (harness?.kind === 'design') {
    const Ctor = lookup(harness.className);
    if (!isFunction(Ctor)) return { error: `No ${harness.className} class defined` };
    return { target: { kind: 'design', className: harness.className, construct: (args) => new (Ctor as unknown as Constructible)(...args) } };
  }
  if (harness?.kind === 'roundTrip') {
    const Ctor = lookup(harness.className);
    if (hasMethod(Ctor, harness.encode) && hasMethod(Ctor, harness.decode)) {
      const fresh = () => new (Ctor as unknown as Constructible)();
      return {
        target: {
          kind: 'roundTrip',
          encodeName: harness.encode,
          encode: (args) => callMethod(fresh(), harness.encode, args, harness.className),
          decode: (encoded) => callMethod(fresh(), harness.decode, [encoded], harness.className),
        },
      };
    }
    const encode = lookup(harness.encode);
    const decode = lookup(harness.decode);
    if (isFunction(encode) && isFunction(decode)) {
      return { target: { kind: 'roundTrip', encodeName: harness.encode, encode: (args) => encode(...args), decode: (encoded) => decode(encoded) } };
    }
    return { error: `No ${harness.className} class or ${harness.encode} and ${harness.decode} functions defined` };
  }
  const Solution = lookup('Solution');
  if (hasMethod(Solution, methodName)) {
    return { target: { kind: 'method', call: (args) => callMethod(new (Solution as unknown as Constructible)(), methodName, args, 'Solution') } };
  }
  const fn = lookup(methodName);
  if (isFunction(fn)) return { target: { kind: 'method', call: (args) => fn(...args) } };
  return { error: `No Solution class or ${methodName} function defined` };
}

type Step = { index: number; name: string } | null;

function runDesign(target: Extract<Target, { kind: 'design' }>, inputs: Record<string, unknown>, partial: unknown[], step: { current: Step }): void {
  const { operations, arguments: argumentLists } = inputs;
  if (!Array.isArray(operations) || !Array.isArray(argumentLists) || !argumentLists.every((args) => Array.isArray(args))) {
    throw new ValueError("a design case needs a list 'operations' and a list of argument lists 'arguments'");
  }
  if (operations.length === 0 || operations.length !== argumentLists.length) {
    throw new ValueError(`operations and arguments must have the same length, at least one (${operations.length} and ${argumentLists.length})`);
  }
  let instance: Record<string, unknown> | undefined;
  operations.forEach((operation: unknown, index) => {
    const name = String(operation);
    const args = argumentLists[index] as unknown[];
    step.current = { index, name };
    if (index === 0) {
      instance = target.construct(args);
      partial.push(null);
    } else {
      partial.push(callMethod(instance as Record<string, unknown>, name, args, target.className));
    }
  });
  step.current = null;
}

const copyOf = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const intersects = (a: Set<unknown>, b: Set<unknown>): boolean => [...a].some((node) => b.has(node));

function graphNodes(named: Record<string, unknown>, paramTypes: HarnessConfig['paramTypes']): Set<unknown> {
  const out = new Set<unknown>();
  for (const name of Object.keys(paramTypes)) {
    if (paramTypes[name] === 'GraphNode') for (const node of reachable(named[name])) out.add(node);
  }
  return out;
}

function stackOf(thrown: unknown): string | undefined {
  try {
    const stack = (thrown as { stack?: unknown } | null)?.stack;
    return typeof stack === 'string' ? stack.slice(0, MAX_STACK) : NO_STACK;
  } catch {
    return NO_STACK;
  }
}

type Failure = { error: string; stack?: string };

/** Runs `work` and answers what it raised, or null: the case's error is a row, never an exception. */
function attempt(work: () => void): Failure | null {
  try {
    work();
    return null;
  } catch (thrown) {
    const stack = stackOf(thrown);
    return { error: describeError(thrown), ...(stack === undefined ? {} : { stack }) };
  }
}

export type CaseEnvironment = {
  config: HarnessConfig;
  target: Target;
  capture: Capture;
  /** reads the learner's own ListNode, TreeNode and graph node classes where it defined them */
  constructors?: () => NodeConstructors;
  now(): number;
};

/** Runs one case: builds the arguments, calls the learner's code, reads the answer back and compares it. Never throws: whatever was raised is the row's error. */
export function runCase(env: CaseEnvironment, testCase: TestCase): CaseRow {
  const { config, target, capture } = env;
  const inputs = testCase.inputs ?? {};
  const expected = testCase.expected === undefined ? null : testCase.expected;
  const started = env.now();
  const partial: unknown[] = [];
  const step: { current: Step } = { current: null };
  let actual: unknown = null;
  let pass = false;
  const failure = attempt(() => {
    const constructors = env.constructors?.() ?? DEFAULT_CONSTRUCTORS;
    if (target.kind === 'design') {
      actual = partial;
      runDesign(target, copyOf(inputs), partial, step);
    } else if (target.kind === 'roundTrip') {
      const paramKind = Object.values(config.paramTypes)[0];
      const { args } = buildArgs(copyOf(inputs), config.paramTypes, constructors);
      const built = paramKind === 'TreeNode' ? reachable(args[0]) : new Set<unknown>();
      const encoded = target.encode(args);
      if (typeof encoded !== 'string') throw new TypeError(`${target.encodeName} must return a string, got ${encoded === null ? 'null' : typeof encoded}`);
      const decoded = target.decode(encoded);
      if (intersects(built, reachable(decoded))) throw new ValueError('the returned tree is the input tree, not a decoded copy');
      actual = readValue(paramKind, decoded);
    } else {
      const { args, named } = buildArgs(copyOf(inputs), config.paramTypes, constructors);
      const given = config.returnType === 'GraphNode' ? graphNodes(named, config.paramTypes) : new Set<unknown>();
      const returned = target.call(args);
      const place = config.inPlace;
      if (place === null || place === undefined) {
        if (given.size > 0 && intersects(given, reachable(returned))) throw new ValueError('the returned graph shares a node with the input: build copies');
        actual = readValue(config.returnType, returned);
      } else {
        actual = readValue(config.paramTypes[place], named[place]);
      }
    }
    pass = compareValues(actual, expected, testCase.compareMode);
  });
  const error = failure && step.current ? `${failure.error} (operation ${step.current.index}: ${step.current.name})` : (failure?.error ?? null);
  const row: CaseRow = {
    pass,
    input: inputs,
    actual: toWire(actual),
    expected,
    stdout: capture.stdout,
    stderr: capture.stderr,
    error,
    elapsed_ms: Math.round((env.now() - started) * 100) / 100,
  };
  if (failure?.stack !== undefined) row.stack = failure.stack;
  return row;
}
