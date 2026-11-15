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
    {
      code: `
        try { return parse(x); }
        catch (e) {
          return { valid: false, reason: e instanceof Error ? e.message : String(e) };
        }
      `,
    },
    {
      code: `
        try { await refresh(); }
        catch (e) {
          setError(e instanceof Error ? e : new Error(String(e)));
        }
      `,
    },
    {
      code: `
        try { return require('peer'); }
        catch {
          const mem = new Map();
          return { getItem: (k) => mem.get(k) };
        }
      `,
    },
    {
      code: `
        try { await run(); }
        catch (e) {
          this.logger.withMetadata({ err: String(e) }).info('run.failed');
        }
      `,
    },
  ],
  invalid: [
    {
      code: `
        try { return parse(x); }
        catch (e) {
          return null;
        }
      `,
      errors: [{ messageId: 'noUnhandledCatch' }],
    },
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
