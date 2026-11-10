// The alias covers vite-resolved imports; inlined CJS deps still `require('react-native')`, so the
// stub's own Module is also registered under the real entry. Global `expect` lets RNTL register its matchers.
import { createRequire } from 'node:module';

import { expect } from 'vitest';

Object.assign(globalThis, { __DEV__: true, expect });

const localRequire = createRequire(import.meta.url);
const stubPath = localRequire.resolve('./vitest.react-native.stub.cjs');
localRequire(stubPath);
const stubModule = localRequire.cache[stubPath];
if (!stubModule) {
  throw new Error('react-native stub did not register in require.cache.');
}
localRequire.cache[localRequire.resolve('react-native')] = stubModule;
