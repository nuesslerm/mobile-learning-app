// Runs the synced `.opencode/hooks.json` (rendered by agent-config-sync from
// `.agents/config/hooks.json`) as an opencode plugin, with the same stdin
// payload and exit-code semantics the hooks get under Claude Code. All the
// logic lives in the shared runtime; this file only points it at the repo's
// hooks file. Auto-discovered by opencode from `.opencode/plugin/`.
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createOpencodeHooksPlugin } from '../../libraries/hook-runtime/src/opencode-adapter.mjs';

const here = dirname(fileURLToPath(import.meta.url));

export default createOpencodeHooksPlugin({
  hooksFile: resolve(here, '..', 'hooks.json'),
  projectRoot: resolve(here, '..', '..'),
});
