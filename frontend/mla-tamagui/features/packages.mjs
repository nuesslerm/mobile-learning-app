// The workspace packages nested in this directory (a package.json beside their sources), as paths from the app root.
// The app's own lint and tests skip them: each package checks itself with its own config.
import { existsSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

export function featurePackages() {
  return readdirSync(here, { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isDirectory() &&
        existsSync(join(here, entry.name, 'package.json'))
    )
    .map((entry) => `features/${entry.name}`)
    .sort();
}
