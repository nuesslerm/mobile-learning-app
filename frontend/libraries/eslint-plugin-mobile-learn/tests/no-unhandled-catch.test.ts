import { describe, it } from 'vitest';
import { RuleTester } from 'oxlint/plugins-dev';

import rule from '../src/rules/no-unhandled-catch.js';

RuleTester.describe = describe;
RuleTester.it = it;

const ruleTester = new RuleTester();

ruleTester.run('no-unhandled-catch', rule as unknown as Parameters<typeof ruleTester.run>[1], {
  valid: [
    {
      code: `
        try { await doSomething(); }
        catch (e) {
          if (!isExpected(e)) throw e;
          await fallback();
        }
      `,
    },
    {
      code: `
        try { await doSomething(); }
        catch (e) {
          logger.withError(e).error('op.failed');
        }
      `,
    },
    {
      code: `
        try { await doSomething(); }
        catch (e) {
          logger.withMetadata({ err: e }).warn('op.failed');
        }
      `,
    },
  ],
  invalid: [
    {
      code: `
        try { await doSomething(); }
        catch (e) {
          await fallback();
        }
      `,
      errors: [{ messageId: 'noUnhandledCatch' }],
    },
    {
      code: `try { await doSomething(); } catch (e) {}`,
      errors: [{ messageId: 'noUnhandledCatch' }],
    },
  ],
});
