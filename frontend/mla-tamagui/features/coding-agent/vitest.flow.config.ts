// Screen-level flow tests (`src/**/*.flow.test.tsx`): whole screens rendered
// through react-native-web in jsdom by @lib/flow-test-harness. Run with
// `pnpm run test:flow`; the plain unit config excludes these files.
import { fileURLToPath } from 'node:url';
import path from 'node:path';

import { flowTestConfig } from '@lib/flow-test-harness/vitest';

export default flowTestConfig({
  packageDir: path.dirname(fileURLToPath(import.meta.url)),
});
