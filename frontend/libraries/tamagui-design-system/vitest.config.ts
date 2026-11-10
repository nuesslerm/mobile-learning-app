// Vitest config for the `@lib/tamagui-design-system` workspace package.
//
// Unit gates run from this package dir (e.g. `pnpm exec vitest run
// theme/__tests__/tokens.unit.test.ts`), so the repo root must be the vitest
// root for the repo-root include patterns to resolve.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  root: '../../..',
  test: {
    include: ['frontend/libraries/tamagui-design-system/**/*.test.{ts,tsx}'],
    environment: 'node',
  },
});
