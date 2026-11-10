import { fileURLToPath } from 'node:url';
import path from 'node:path';

import { flowTestConfig } from './src/vitest/flowTestConfig';

export default flowTestConfig({
  packageDir: path.dirname(fileURLToPath(import.meta.url)),
  overrides: { test: { include: ['src/**/*.test.{ts,tsx}'] } },
});
