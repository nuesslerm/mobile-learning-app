import { execFileSync } from 'node:child_process';

// Builds the release binary unless another implementation is named.
export default function setup(): void {
  if (process.env['LEETCODE_BACKEND_CMD']) return;
  execFileSync(
    'cargo',
    ['build', '--release', '--locked', '-p', 'leetcode-backend-rs', '--manifest-path', 'Cargo.toml'],
    { cwd: new URL('../..', import.meta.url).pathname, stdio: 'ignore' }
  );
}
