// The harness that runs inside the worker. scripts/js-runner-build.mjs compiles it to one line, the prelude, and
// workerSource.ts puts that line in front of the learner's code: the worker's script is the learner's own, so its top-level
// var, function and class are the worker's globals and its line numbers are exact.
import { createCapture, createConsole, createProcess } from './capture';
import { resolveTarget, runCase, type Target } from './caseRunner';
import { DEFAULT_CONSTRUCTORS, GraphNode, ListNode, TreeNode, type NodeConstructors } from './nodes';
import { CONFIG_KEY, LOADED_KEY, type FromWorker, type HarnessConfig, type ToWorker } from '../workerProtocol';

type Scope = Record<PropertyKey, unknown> & {
  postMessage(message: unknown): void;
  onmessage: ((event: { data: unknown }) => void) | null;
};

const define = (scope: Scope, key: PropertyKey, value: unknown) => {
  Object.defineProperty(scope, key, { value, configurable: true, writable: true, enumerable: false });
};

// a top-level function or var of the learner's is hoisted above this line and cannot be redefined: it is the one the harness builds with
const provide = (scope: Scope, name: string, value: unknown) => {
  if (!Object.prototype.hasOwnProperty.call(scope, name)) define(scope, name, value);
};

// The one place a name is read from the learner's code at run time: a class or let at the top level is not a property of the global object.
function lookupName(name: string): unknown {
  if (!/^[A-Za-z_$][\w$]*$/.test(name)) return undefined;
  // oxlint-disable-next-line no-eval -- an indirect eval is the only way to read a global lexical binding by name
  return (0, eval)(`typeof ${name}==='undefined'?undefined:${name}`) as unknown;
}

const isConstructor = (value: unknown): boolean => typeof value === 'function';

function constructorsOf(lookup: (name: string) => unknown): NodeConstructors {
  const pick = <T>(name: string, fallback: T): T => {
    const own = lookup(name);
    return isConstructor(own) ? (own as T) : fallback;
  };
  const graph = [lookup('_Node'), lookup('Node')].find((value) => isConstructor(value) && value !== GraphNode);
  return {
    ListNode: pick('ListNode', DEFAULT_CONSTRUCTORS.ListNode),
    TreeNode: pick('TreeNode', DEFAULT_CONSTRUCTORS.TreeNode),
    GraphNode: (graph as NodeConstructors['GraphNode'] | undefined) ?? DEFAULT_CONSTRUCTORS.GraphNode,
  };
}

export function startHarness(scope: Scope, config: HarnessConfig, lookup: (name: string) => unknown = lookupName): void {
  const post = (message: FromWorker) => scope.postMessage(message);
  const now = () => performance.now();
  let capture = createCapture(config.outputCap);
  const boot = capture;
  let target: Target | undefined;

  define(scope, 'console', createConsole(() => capture));
  define(scope, 'process', createProcess(() => capture));
  provide(scope, 'ListNode', ListNode);
  provide(scope, 'TreeNode', TreeNode);
  provide(scope, '_Node', GraphNode);
  provide(scope, 'Node', GraphNode);

  scope.onmessage = (event) => {
    const message = event.data as ToWorker;
    if (message?.type !== 'case' || !target) return;
    capture = createCapture(config.outputCap);
    post({ type: 'begin', index: message.index });
    const row = runCase({ config, target, capture, constructors: () => constructorsOf(lookup), now }, message.case);
    post({ type: 'row', index: message.index, row });
  };

  const loadedKey = Symbol.for(LOADED_KEY);
  define(scope, loadedKey, () => {
    delete scope[loadedKey];
    const found = resolveTarget(config, lookup);
    if ('error' in found) {
      post({ type: 'loaded', stdout: boot.stdout, error: found.error });
      return;
    }
    target = found.target;
    post({ type: 'loaded', stdout: boot.stdout });
  });
}

const scope = globalThis as unknown as Scope;
const configKey = Symbol.for(CONFIG_KEY);
const config = scope[configKey] as HarnessConfig | undefined;
if (config) {
  delete scope[configKey];
  startHarness(scope, config);
}
