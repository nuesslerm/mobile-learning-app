import type { Node, Statement, IfStatement, BlockStatement, CatchClause } from 'estree';

import type { Rule } from '@/rules/index.js';

function containsThrow(node: Statement): boolean {
  if (node.type === 'ThrowStatement') return true;
  if (node.type === 'IfStatement') {
    const ifNode = node as IfStatement;
    return (
      containsThrow(ifNode.consequent) ||
      (ifNode.alternate != null && containsThrow(ifNode.alternate as Statement))
    );
  }
  if (node.type === 'BlockStatement') {
    return (node as BlockStatement).body.some(containsThrow);
  }
  return false;
}

function containsLoggerCall(node: Statement | undefined): boolean {
  if (!node) return false;

  let found = false;
  // Walk the statement body looking for a logger call that references the
  // catch parameter (or any logger call if no param name is provided).
  function walk(n: Node): void {
    if (found) return;
    if (n.type === 'CallExpression') {
      const callee = n.callee;
      // logError(logger, err, ...) / logWarnError(logger, err, ...)
      if (callee.type === 'Identifier' && /^(logError|logWarnError)$/.test(callee.name)) {
        found = true;
        return;
      }
      if (callee.type === 'MemberExpression') {
        const obj = callee.object;
        // logger.withError(err) / logger.withMetadata({ err }) / log.error(...)
        if (obj.type === 'MemberExpression' && obj.property.type === 'Identifier' && /^(logger|log)$|Log(ger)?$/.test(obj.property.name)) {
          found = true;
          return;
        }
        if (
          obj.type === 'Identifier' &&
          (obj.name === 'logger' ||
            obj.name === 'log' ||
            /Log(ger)?$/.test(obj.name))
        ) {
          found = true;
          return;
        }
        // chained .withError(err).error(...)
        if (obj.type === 'CallExpression') {
          walk(obj);
          if (found) return;
        }
      }
    }
    for (const key of Object.keys(n)) {
      if (key === 'parent') continue;
      const value = (n as unknown as Record<string, unknown>)[key];
      if (Array.isArray(value)) {
        for (const item of value) {
          if (item && typeof (item as Node).type === 'string') {
            walk(item as Node);
          }
        }
      } else if (value && typeof (value as Node).type === 'string') {
        walk(value as Node);
      }
    }
  }
  walk(node);
  return found;
}

function children(n: Node): Node[] {
  const out: Node[] = [];
  for (const key of Object.keys(n)) {
    if (key === 'parent') continue;
    const value = (n as unknown as Record<string, unknown>)[key];
    for (const item of Array.isArray(value) ? value : [value]) {
      if (item && typeof (item as Node).type === 'string') out.push(item as Node);
    }
  }
  return out;
}

function references(n: Node, name: string): boolean {
  if (n.type === 'Identifier') return n.name === name;
  return children(n).some((c) => references(c, name));
}

// The error is handed on instead of swallowed: returned in a value, or given to a state setter (`setError(err)`).
function handsOnError(n: Node, name: string | undefined): boolean {
  if (n.type === 'ReturnStatement' && n.argument) {
    const a = n.argument;
    // a fallback built from real work (an in-memory store, a computed value) is a handled failure; `return null` is a swallow
    const trivial = a.type === 'Literal' || (a.type === 'Identifier' && a.name === 'undefined');
    return (name !== undefined && references(a, name)) || !trivial;
  }
  if (n.type === 'CallExpression' && n.callee.type === 'Identifier' && /^set[A-Z]/.test(n.callee.name)) {
    return name !== undefined && n.arguments.some((a) => references(a as Node, name));
  }
  return children(n).some((c) => handsOnError(c, name));
}

const rule: Rule = {
  meta: {
    type: 'problem',
    docs: {
      description:
        'Catch blocks must either re-throw unexpected errors or log them via a logger. Silent catches hide failures.',
    },
    schema: [],
    messages: {
      noUnhandledCatch:
        'Catch block does not re-throw or log the error. Either re-throw (if unexpected) or log with logger.withError(err).error("...") / logger.withMetadata({ err }).warn("...").',
    },
  },
  create(context) {
    return {
      CatchClause(node: Node) {
        const catchNode = node as CatchClause;
        const hasThrow = catchNode.body.body.some(containsThrow);
        const hasLog = catchNode.body.body.some((stmt) => containsLoggerCall(stmt));
        const name = catchNode.param?.type === 'Identifier' ? catchNode.param.name : undefined;
        const handsOn = catchNode.body.body.some((stmt) => handsOnError(stmt, name));
        if (!hasThrow && !hasLog && !handsOn) {
          context.report({
            node: node as unknown as { range: [number, number] },
            messageId: 'noUnhandledCatch',
          });
        }
      },
    };
  },
};

export default rule;
