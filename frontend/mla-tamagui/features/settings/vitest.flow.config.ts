// Screen-level flow tests (`src/**/*.flow.test.tsx`): whole screens rendered
// through react-native-web in jsdom by @lib/flow-test-harness. Run with
// `pnpm run test:flow`; the plain unit config excludes these files.
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { flowTestConfig } from '@lib/flow-test-harness/vitest';

const config = flowTestConfig({
  packageDir: path.dirname(fileURLToPath(import.meta.url)),
});

export { config as default };
