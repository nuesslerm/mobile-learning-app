// Babel plugin: auto-injects call-site metadata (file, line, enclosing
// function/component name, and the full lexical nesting chain) into every
// `Logger.<method>(...)` call, so authoring code never names its module,
// component or function just to tag where a log came from.
//
// Rewrites the FIRST member access on the `Logger` identifier in a call
// chain — `Logger.info(...)` becomes `Logger.withContext({...}).info(...)`,
// `Logger.withError(err).error(...)` becomes
// `Logger.withContext({...}).withError(err).error(...)` — so it composes
// with every existing chain method without special-casing per-method.
//
// This only rewrites `Logger` when it resolves to an IMPORTED binding (not
// a locally shadowed variable), and only when it's immediately followed by
// one of the known API methods being CALLED (not merely referenced, e.g.
// `export { Logger }` is left untouched). `path.get('object').replaceWith(...)`
// requeues the newly-inserted `Logger.withContext(...)` for traversal, and
// its own `Logger` identifier would otherwise re-match this exact visitor —
// the injected identifier is marked (`INJECTED` flag) so that re-visit is a
// guaranteed no-op instead of infinite recursion.
//
// `fnPath` is LEXICAL nesting (which function is defined inside which, in
// source), not the RUNTIME call stack (which function actually invoked
// which). A callback defined inside `ScreenA` but invoked from `ScreenB`
// still reports `"ScreenA > ..."` — Babel only sees source structure, never
// the real invocation chain.
//
// This is the static-call-site half of location tagging. It does NOT
// replace context that only exists at runtime (arbitrary per-instance
// values); use `Logger.withMetadata({...})` at the call site for that, same
// as today.
//
// `testID` is injected for every call site, repo-wide: it resolves a
// `testID` (or `props.testID`, for the `...props` rest-param shape) binding
// already in scope, falling back to `null` when neither exists. This can
// produce `testID: undefined` at runtime for a `props` binding that has no
// `testID` field — accepted as harmless noise in exchange for not having to
// special-case which files the convention applies to.
//
// Authored in TypeScript but consumed by Metro's babel.config.js as a
// compiled CJS artifact (`pnpm run build:babel` -> dist/babel/, wired into
// the repo's root postinstall) — babel.config.js is `require()`d by Node
// before any TS transform runs, so the source .ts file itself isn't directly
// requireable there.

import { relative } from 'node:path';

import type {
  NodePath,
  PluginObj,
  PluginPass,
  types as BabelTypes,
} from '@babel/core';

const LOGGER_METHODS = new Set([
  'info',
  'debug',
  'warn',
  'error',
  'withMetadata',
  'withError',
  'withContext',
]);

const INJECTED = Symbol('loggerSourceInjected');
type MarkedIdentifier = BabelTypes.Identifier & { [INJECTED]?: true };

const ANONYMOUS = '<anonymous>';

/** Resolve a single function-like NodePath's display name, or null if it can't be named. */
function resolveFunctionName(
  fnPath: NodePath,
  t: typeof BabelTypes
): string | null {
  const fnNode = fnPath.node as BabelTypes.Function;

  // Named function declaration/expression: `function foo() {}`.
  if ('id' in fnNode && fnNode.id && t.isIdentifier(fnNode.id)) {
    return fnNode.id.name;
  }

  // Class method / object method shorthand: Babel classifies both as
  // "Function" nodes, so `fnPath` IS the method itself here (not its
  // parent) — the name lives on the method node's own `.key`.
  if (
    (t.isClassMethod(fnNode) ||
      t.isClassPrivateMethod(fnNode) ||
      t.isObjectMethod(fnNode)) &&
    t.isIdentifier(fnNode.key)
  ) {
    return fnNode.key.name;
  }

  // Anonymous function/arrow expression: look at what it's being assigned to.
  const parent = fnPath.parentPath;
  if (!parent) return null;

  if (parent.isVariableDeclarator() && t.isIdentifier(parent.node.id)) {
    return parent.node.id.name;
  }
  if (parent.isObjectProperty() && t.isIdentifier(parent.node.key)) {
    return parent.node.key.name;
  }
  if (parent.isClassProperty() && t.isIdentifier(parent.node.key)) {
    return parent.node.key.name;
  }
  return null;
}

/**
 * Walk every enclosing function-like ancestor, innermost first. Each level
 * that can't be named (e.g. an inline callback argument with no assignment
 * to read a name from) is represented as `<anonymous>` rather than skipped,
 * so the chain's nesting DEPTH is always accurate even when a segment's
 * NAME isn't resolvable.
 */
function findEnclosingFunctionNames(
  path: NodePath,
  t: typeof BabelTypes
): string[] {
  const namesInnerToOuter: string[] = [];
  let current: NodePath = path;
  for (;;) {
    const fnPath = current.findParent((p) => p.isFunction());
    if (!fnPath) break;
    namesInnerToOuter.push(resolveFunctionName(fnPath, t) ?? ANONYMOUS);
    current = fnPath;
  }
  return namesInnerToOuter;
}

/**
 * Resolve a `testID` expression from what's already bound in scope at the
 * call site: a destructured `testID` param (bare identifier), or a
 * `...props` rest binding (`props.testID`). Returns null if neither is
 * found.
 */
function findTestIdExpression(
  path: NodePath,
  t: typeof BabelTypes
): BabelTypes.Expression | null {
  if (path.scope.getBinding('testID')) {
    return t.identifier('testID');
  }
  if (path.scope.getBinding('props')) {
    return t.memberExpression(t.identifier('props'), t.identifier('testID'));
  }
  return null;
}

export function injectLoggerSource({
  types: t,
}: {
  types: typeof BabelTypes;
}): PluginObj<PluginPass> {
  return {
    name: 'inject-logger-source',
    visitor: {
      MemberExpression(path, state) {
        const { node } = path;

        if (!t.isIdentifier(node.object, { name: 'Logger' })) return;
        if ((node.object as MarkedIdentifier)[INJECTED]) return;
        if (
          !t.isIdentifier(node.property) ||
          !LOGGER_METHODS.has(node.property.name)
        ) {
          return;
        }
        if (!path.parentPath.isCallExpression({ callee: node })) return;

        const binding = path.scope.getBinding('Logger');
        if (!binding || binding.kind !== 'module') return;

        const filename = state.filename;
        const cwd = state.cwd ?? process.cwd();
        const file = filename ? relative(cwd, filename) : 'unknown';
        const line = node.loc ? node.loc.start.line : 0;
        const namesInnerToOuter = findEnclosingFunctionNames(path, t);
        const fn = namesInnerToOuter.find((name) => name !== ANONYMOUS) ?? null;
        const fnPath =
          namesInnerToOuter.length > 0
            ? [...namesInnerToOuter].reverse().join(' > ')
            : null;

        const testIdExpr = findTestIdExpression(path, t);

        const sourceObj = t.objectExpression([
          t.objectProperty(t.identifier('file'), t.stringLiteral(file)),
          t.objectProperty(t.identifier('line'), t.numericLiteral(line)),
          t.objectProperty(
            t.identifier('fn'),
            fn ? t.stringLiteral(fn) : t.nullLiteral()
          ),
          t.objectProperty(
            t.identifier('fnPath'),
            fnPath ? t.stringLiteral(fnPath) : t.nullLiteral()
          ),
          t.objectProperty(
            t.identifier('testID'),
            testIdExpr ?? t.nullLiteral()
          ),
        ]);

        const injectedLoggerId: MarkedIdentifier = t.identifier('Logger');
        injectedLoggerId[INJECTED] = true;

        path
          .get('object')
          .replaceWith(
            t.callExpression(
              t.memberExpression(injectedLoggerId, t.identifier('withContext')),
              [sourceObj]
            )
          );
      },
    },
  };
}

export default injectLoggerSource;
