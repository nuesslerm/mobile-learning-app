#!/usr/bin/env node
/* global __dirname */
const { execFileSync, spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

function readArg(name, shortName) {
  const longPrefix = `${name}=`;
  for (let index = 2; index < process.argv.length; index += 1) {
    const value = process.argv[index];

    if (value === name || value === shortName) {
      return process.argv[index + 1];
    }

    if (value.startsWith(longPrefix)) {
      return value.slice(longPrefix.length);
    }
  }

  return undefined;
}

function getCurrentGitBranch() {
  return execFileSync('git', ['branch', '--show-current'], {
    encoding: 'utf8',
  }).trim();
}

function getLatestCommitMessage() {
  return execFileSync('git', ['log', '-1', '--pretty=%s'], {
    encoding: 'utf8',
  }).trim();
}

const branch =
  readArg('--branch', '-b') ||
  process.env.EAS_UPDATE_BRANCH ||
  getCurrentGitBranch();
const message =
  readArg('--message', '-m') ||
  process.env.EAS_UPDATE_MESSAGE ||
  getLatestCommitMessage();
// Default to iOS only. `eas update --platform` accepts ios | android | all, so
// publishing iOS inherently excludes Android (and web is never published by
// `eas update`). Override with --platform/-p or EAS_UPDATE_PLATFORM to opt into more.
const platform =
  readArg('--platform', '-p') || process.env.EAS_UPDATE_PLATFORM || 'ios';

// REQUIRED: every preview must cold-start on the screen under review, never the
// Home tab (see AGENTS.md). The route MUST be passed explicitly via
// --initial-route / -r; the script then inlines it into the export below as
// EXPO_PUBLIC_INITIAL_ROUTE so the published bundle boots there.
const initialRoute = readArg('--initial-route', '-r');

const allowedPlatforms = ['ios', 'android', 'all'];

if (!branch) {
  console.error(
    'No EAS Update branch was provided. Check out a named git branch or pass --branch <name>.'
  );
  process.exit(1);
}

if (!initialRoute) {
  console.error(
    'Missing required --initial-route. EAS previews must cold-start on the screen\n' +
      'under review, not Home, so the route has to be passed explicitly:\n' +
      '  pnpm run publish:eas -- --initial-route /your-route'
  );
  process.exit(1);
}

if (!allowedPlatforms.includes(platform)) {
  console.error(
    `Invalid platform "${platform}". Use one of: ${allowedPlatforms.join(', ')}.`
  );
  process.exit(1);
}

// Non-interactive auth: EXPO_TOKEN in the environment (`wf ship eas` passes it from the vault), else an interactive
// eas-cli login session, which `eas whoami` verifies.
if (!process.env.EXPO_TOKEN) {
  const whoami = spawnSync('npx', ['eas-cli', 'whoami', '--non-interactive'], {
    encoding: 'utf8',
  });
  if (whoami.status !== 0) {
    console.error(
      'No EAS auth. Any one of these works:\n' +
        '  - export EXPO_TOKEN=<access token from expo.dev -> Account -> Access Tokens>\n' +
        '  - publish through `wf ship eas`, which reads it from the vault\n' +
        '  - log in interactively: `npx eas-cli login`'
    );
    process.exit(1);
  }
}

// Export locally first, THEN publish that export with `eas update --skip-bundler`.
// `eas update` does its own bundling and does NOT inline ad-hoc local
// `EXPO_PUBLIC_*` shell vars (it resolves env from EAS / .env), so a plain
// `eas update` would drop e.g. EXPO_PUBLIC_APP_ENV. Running `expo export`
// ourselves (which inherits this process's env) inlines them correctly, and
// `--skip-bundler` makes eas upload exactly that build.
const OUTPUT_DIR = 'dist';

// Force the resolved route into the export env so it inlines into the bundle.
// The appConfig override applies in any appEnv, so EXPO_PUBLIC_INITIAL_ROUTE
// alone guarantees the cold-start route regardless of EXPO_PUBLIC_APP_ENV.
//
// ENVIRONMENT EARMARK: remote Expo Go previews (EAS updates) default to
// 'staging'. A published bundle runs with __DEV__ === false, so without this
// the app would resolve appEnv to 'production' and hide every dev affordance
// (DebugFab, dev-runtime-config route override). Local `expo start` stays
// 'development' via __DEV__; a REAL production publish must say so explicitly
// with EXPO_PUBLIC_APP_ENV=production.
const appEnv = process.env.EXPO_PUBLIC_APP_ENV ?? 'staging';
const exportEnv = {
  ...process.env,
  EXPO_PUBLIC_APP_ENV: appEnv,
  EXPO_PUBLIC_INITIAL_ROUTE: initialRoute,
};

console.log(`[publish:eas] cold-start initial route: ${initialRoute}`);
console.log(`[publish:eas] appEnv earmark: ${appEnv}`);

// expo export's `all` adds web when the app config has it; an EAS Update carries only ios and android
const exportPlatforms = platform === 'all' ? ['ios', 'android'] : [platform];
const seconds = (fromMs) => Math.round((Date.now() - fromMs) / 100) / 10;
const exportStarted = Date.now();
// The export is the cached Nx target `export:eas` (outputs `dist`); the upload below stays a side effect that runs every time.
const exportResult = spawnSync('pnpm', ['exec', 'nx', 'run', 'mla-tamagui:export:eas'], {
  stdio: 'inherit',
  env: {
    ...exportEnv,
    EXPO_EXPORT_PLATFORM_FLAGS: exportPlatforms.flatMap((p) => ['--platform', p]).join(' '),
    // hermes-compiler ships no Linux arm64 hermesc, so the CI runner (an arm64 VM) publishes plain JS, which Hermes runs as is
    EXPO_EXPORT_FLAGS: process.platform === 'linux' && process.arch === 'arm64' ? '--no-bytecode' : '',
  },
});

if (exportResult.status !== 0) {
  process.exit(exportResult.status ?? 1);
}
const exportS = seconds(exportStarted);

const uploadStarted = Date.now();
const result = spawnSync(
  'npx',
  [
    'eas-cli',
    'update',
    '--branch',
    branch,
    '--message',
    message,
    '--platform',
    platform,
    '--skip-bundler',
    '--input-dir',
    OUTPUT_DIR,
    '--environment',
    process.env.EAS_UPDATE_ENVIRONMENT || 'preview',
    '--non-interactive',
    '--json',
  ],
  { stdio: ['inherit', 'pipe', 'inherit'], encoding: 'utf8' }
);

const uploadS = seconds(uploadStarted);

// The names the bundle baked in (never their values) and the hosts its URLs name, which the device must resolve.
const baked = Object.keys(exportEnv)
  .filter((name) => name.startsWith('EXPO_PUBLIC_'))
  .sort();
const hostOf = (url) => {
  try {
    return new URL(url).hostname;
  } catch {
    return '';
  }
};
const hosts = [
  ...new Set(
    baked.filter((name) => name.endsWith('_URL')).map((name) => hostOf(exportEnv[name]))
  ),
].filter(Boolean);

// eas update --json prints the published updates; wf ship eas reads their group id and the export's facts from these files
if (result.status === 0) {
  process.stdout.write(result.stdout);
  fs.writeFileSync(path.join(OUTPUT_DIR, 'eas-update.json'), result.stdout);
  fs.writeFileSync(
    path.join(OUTPUT_DIR, 'eas-update-export.json'),
    JSON.stringify({ platform, env: baked, hosts, exportS, uploadS })
  );
}

process.exit(result.status ?? 1);
