import { execFileSync } from 'node:child_process';

// Builds the release binary of the service unless another implementation is named.
export default function setup(): void {
  if (process.env['RCA_BACKEND_CMD']) return;
  execFileSync('cargo', ['build', '--release', '--manifest-path', 'Cargo.toml', '-p', 'rca-service'], {
    cwd: new URL('../..', import.meta.url).pathname,
    stdio: 'ignore',
  });
}
