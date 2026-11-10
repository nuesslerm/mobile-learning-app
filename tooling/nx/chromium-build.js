// The Chromium the browser tests drive, named by the Playwright version the lockfile pins when there is one: the sandbox ships its own newer
// build (/opt/pw-browsers) and CI installs the pinned one, so a hash that followed the installed build never matched across them and CI
// re-ran every browser test the sandbox had already passed. Without a pinned Playwright it falls back to the installed build.
// Looks where `cdp::launch::find_chromium` looks (WF_CHROMIUM, then the newest Playwright-installed `chromium-<build>`).
const { execFileSync } = require('node:child_process');
const { readdirSync, readFileSync } = require('node:fs');
const { join } = require('node:path');

function pinned() {
  try {
    const lock = readFileSync(join(process.cwd(), 'pnpm-lock.yaml'), 'utf8');
    const found = /^ {2}playwright-core@([^:\s]+):/m.exec(lock);
    return found ? `playwright-core@${found[1]}` : null;
  } catch {
    return null;
  }
}

function installed() {
  const roots = [
    process.env.PLAYWRIGHT_BROWSERS_PATH,
    '/opt/pw-browsers',
    join(process.env.HOME ?? '', '.cache/ms-playwright'),
  ].filter((root) => root && root !== '0');
  for (const root of roots) {
    try {
      const builds = readdirSync(root)
        .filter((name) => name.startsWith('chromium-'))
        .sort();
      if (builds.length) return builds[builds.length - 1];
    } catch {
      // not installed here
    }
  }
  return 'none';
}

function build() {
  if (process.env.WF_CHROMIUM) {
    try {
      return execFileSync(process.env.WF_CHROMIUM, ['--version'], { encoding: 'utf8' }).trim();
    } catch {
      return 'unreadable';
    }
  }
  return pinned() ?? installed();
}

console.log(build());
