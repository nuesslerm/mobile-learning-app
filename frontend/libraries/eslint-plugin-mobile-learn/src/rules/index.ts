import noHermesUnsupportedApi from '@/rules/no-hermes-unsupported-api.js';
import noUnhandledCatch from '@/rules/no-unhandled-catch.js';
import type { Node } from 'estree';

export interface RuleMeta {
  type: 'problem' | 'suggestion' | 'layout';
  docs: {
    description: string;
  };
  schema: [];
  messages: Record<string, string>;
}

export interface RuleContext {
  report(
    diagnostic: {
      node: { range: [number, number] };
      messageId: string;
      data?: Record<string, string | number | boolean | null | undefined>;
    },
  ): void;
}

export interface Rule {
  meta: RuleMeta;
  create(context: RuleContext): { [key: string]: (node: Node) => void };
}

const rules: Record<string, Rule> = {
  'no-hermes-unsupported-api': noHermesUnsupportedApi,
  'no-unhandled-catch': noUnhandledCatch,
};

export default rules;
