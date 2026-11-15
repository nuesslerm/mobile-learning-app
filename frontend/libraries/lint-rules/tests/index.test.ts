import { describe, it, expect } from 'vitest';
import rules from '../src/rules/index.js';

describe('eslint-plugin ("./src/index.ts")', () => {
  const ruleKeys = [
    'no-hermes-unsupported-api',
    'no-unhandled-catch',
  ];
  const eslintPluginRuleKeys = Object.keys(rules);

  it('exports all available rules', () => {
    expect([...eslintPluginRuleKeys].sort()).toEqual([...ruleKeys].sort());
  });
});
