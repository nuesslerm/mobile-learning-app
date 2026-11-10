import { describe, it } from 'vitest';
import { RuleTester } from 'oxlint/plugins-dev';

import rule from '../src/rules/no-hermes-unsupported-api.js';

RuleTester.describe = describe;
RuleTester.it = it;

const ruleTester = new RuleTester();

ruleTester.run('no-hermes-unsupported-api', rule as unknown as Parameters<typeof ruleTester.run>[1], {
  valid: [
    {
      code: `
        const controller = new AbortController();
        setTimeout(() => controller.abort(), 90_000);
        fetch(url, { signal: controller.signal });
      `,
      filename: 'valid-abort-controller.ts',
    },
    // Unrelated .timeout()/.any() calls on a different receiver must not
    // false-positive -- only the AbortSignal-qualified form is banned.
    {
      code: `
        const scheduler = { timeout: (ms: number) => ms };
        scheduler.timeout(1000);
      `,
      filename: 'valid-unrelated-timeout.ts',
    },
    {
      code: `
        const results = [1, 2].any((x) => x > 0);
      `,
      filename: 'valid-unrelated-any.ts',
    },
  ],
  invalid: [
    // The Hermes feature matrix (spike entries).
    {
      code: 'const copy = structuredClone(value);',
      filename: 'invalid-structured-clone.ts',
      errors: [{ messageId: 'bannedApi' }],
    },
    {
      code: "const s = new Intl.Segmenter('en');",
      filename: 'invalid-segmenter.ts',
      errors: [{ messageId: 'bannedApi' }],
    },
    {
      code: 'const k = crypto.subtle;',
      filename: 'invalid-subtle.ts',
      errors: [{ messageId: 'bannedApi' }],
    },
    {
      code: 'const r = response.body.getReader();',
      filename: 'invalid-get-reader.ts',
      errors: [{ messageId: 'bannedApi' }],
    },
    {
      code: `
        const signal = AbortSignal.timeout(90_000);
      `,
      filename: 'invalid-abort-signal-timeout.ts',
      errors: [{ messageId: 'bannedApi' }],
    },
    {
      code: `
        const signal = AbortSignal.any([a, b]);
      `,
      filename: 'invalid-abort-signal-any.ts',
      errors: [{ messageId: 'bannedApi' }],
    },
    {
      code: `
        function run(signal: AbortSignal) {
          signal.throwIfAborted();
        }
      `,
      filename: 'invalid-throw-if-aborted.ts',
      errors: [{ messageId: 'bannedApi' }],
    },
  ],
});
