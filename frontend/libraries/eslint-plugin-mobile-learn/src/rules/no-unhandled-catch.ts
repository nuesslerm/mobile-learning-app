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
        if (!hasThrow && !hasLog) {
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
