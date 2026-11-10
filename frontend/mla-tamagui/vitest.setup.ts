// vitest.setup.ts
import { expect } from 'vitest';

// `logger.ts` reads `__DEV__` at module load to default console echo.
// Vitest runs in Node, so define it here before the stub is required.
(globalThis as { __DEV__?: boolean }).__DEV__ = true;

// Expose `expect` as a global so RTL's `build/matchers/extend-expect.js`
// (which calls `expect.extend({...})` at module top level) finds it.
(globalThis as { expect?: typeof expect }).expect = expect;
