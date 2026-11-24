import type { Node } from 'estree';

import type { Rule } from '@/rules/index.js';

const HEADER_KEYS = new Set(['header', 'unstable_headerLeftItems', 'unstable_headerRightItems']);

type Loose = { type: string; [key: string]: unknown };

function isStackScreen(el: Loose): boolean {
  const name = el.name as Loose;
  if (name.type !== 'JSXMemberExpression') return false;
  const object = name.object as Loose;
  const property = name.property as Loose;
  return object.type === 'JSXIdentifier' && object.name === 'Stack' && property.name === 'Screen';
}

const rule: Rule = {
  meta: {
    type: 'problem',
    docs: {
      description:
        'A Stack.Screen declares its header through the design system (ScreenHeader, useScreenHeaderOptions), never with options.header or the unstable header item options.',
    },
    schema: [],
    messages: {
      noDirectScreenHeader:
        'Declare the header through the design system (ScreenHeader or useScreenHeaderOptions from composites/native-header), not with `{{key}}` on a Stack.Screen.',
    },
  },
  create(context) {
    return {
      JSXOpeningElement(node: Node) {
        const el = node as unknown as Loose;
        if (!isStackScreen(el)) return;
        for (const attr of el.attributes as Loose[]) {
          if (attr.type !== 'JSXAttribute') continue;
          const attrName = attr.name as Loose;
          if (attrName.type !== 'JSXIdentifier' || attrName.name !== 'options') continue;
          const value = attr.value as Loose | null;
          if (!value || value.type !== 'JSXExpressionContainer') continue;
          const expression = value.expression as Loose;
          if (expression.type !== 'ObjectExpression') continue;
          for (const prop of expression.properties as Loose[]) {
            if (prop.type !== 'Property') continue;
            const key = prop.key as Loose;
            const keyName = key.type === 'Identifier' ? (key.name as string) : key.type === 'Literal' ? String(key.value) : '';
            if (HEADER_KEYS.has(keyName)) {
              context.report({
                node: prop as unknown as { range: [number, number] },
                messageId: 'noDirectScreenHeader',
                data: { key: keyName },
              });
            }
          }
        }
      },
    };
  },
};

export default rule;
