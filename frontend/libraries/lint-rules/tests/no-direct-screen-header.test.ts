import { describe, it } from 'vitest';
import { RuleTester } from 'oxlint/plugins-dev';

import rule from '../src/rules/no-direct-screen-header.js';

RuleTester.describe = describe;
RuleTester.it = it;

const ruleTester = new RuleTester();

ruleTester.run('no-direct-screen-header', rule as unknown as Parameters<typeof ruleTester.run>[1], {
  valid: [
    { code: 'const a = <Stack.Screen name="x" options={{ title: "T", headerShown: false }} />;', filename: 'a.tsx' },
    { code: 'const a = <Stack.Screen name="x" options={useScreenHeaderOptions(spec)} />;', filename: 'b.tsx' },
    { code: 'const a = <ScreenHeader title="T" />;', filename: 'c.tsx' },
    { code: 'const a = <Other.Screen options={{ header: () => null }} />;', filename: 'd.tsx' },
  ],
  invalid: [
    {
      code: 'const a = <Stack.Screen name="x" options={{ header: () => null }} />;',
      filename: 'e.tsx',
      errors: [{ messageId: 'noDirectScreenHeader' }],
    },
    {
      code: 'const a = <Stack.Screen options={{ unstable_headerLeftItems: () => [], unstable_headerRightItems: () => [] }} />;',
      filename: 'f.tsx',
      errors: [{ messageId: 'noDirectScreenHeader' }, { messageId: 'noDirectScreenHeader' }],
    },
  ],
});
