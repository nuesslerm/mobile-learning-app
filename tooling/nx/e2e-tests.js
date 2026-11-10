// Every e2e test (`e2e/<name>.lua` with a header) as an Nx project with one cached target per platform, keyed by the test's
// own files and the feature code it drives (owner, 18 Oct): a test runs again only when one of those changed.
const { readFileSync, existsSync } = require('node:fs');
const { join, basename } = require('node:path');

const APP = 'frontend/mla-tamagui';

// the device harness that drives a test (A5, 20 Oct): the runner, Lua binder, command table, dispatcher, selectors and golden
// compare both platforms share, and each platform's driver
const HARNESS = {
  shared: [
    'clis/harness-core/src/run.rs',
    'clis/harness-core/src/platform.rs',
    'clis/harness-core/src/device/mod.rs',
    'clis/harness-core/src/device/lua.rs',
    'clis/harness-core/src/device/api.rs',
    'clis/harness-core/src/device/commands.rs',
    'clis/harness-core/src/device/sel.rs',
    'clis/harness-core/src/device/golden.rs',
    'clis/harness-core/src/golden.rs',
    'clis/harness-core/src/pixelmatch.rs',
  ],
  android: ['clis/harness-android/src/*.rs', 'clis/harness-android/src/*.json', 'clis/harness-android/assets/**'],
  browser: [
    'clis/harness-browser/src/*.rs',
    'clis/harness-browser/src/*.js',
    'clis/harness-browser/src/*.json',
    'clis/cdp/src/**',
    // serve the export the page loads from: its rewrites decide which html a route gets, the listener each response's headers
    'clis/pipeline/src/export_site.rs',
    'clis/harness-core/src/staticd.rs',
  ],
};

function header(text) {
  if (!text.startsWith('--[[---\n')) return null;
  const end = text.indexOf('\n---]]');
  if (end < 0) return null;
  const fields = {};
  for (const line of text.slice(8, end).split('\n')) {
    const at = line.indexOf(':');
    if (at > 0) fields[line.slice(0, at).trim()] = line.slice(at + 1).trim();
  }
  return fields;
}

const list = (value) =>
  (value ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);

// `features: leetcode` names a feature under the app: an in-app one (its own project, see its project.json) or a package
function featureProject(root, name) {
  const dir = join(root, APP, 'features', name);
  if (existsSync(join(dir, 'project.json'))) return `feature-${name}`;
  if (existsSync(join(dir, 'package.json')))
    return JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')).name;
  throw new Error(
    `e2e feature "${name}" has no project.json or package.json in ${APP}/features/${name}`
  );
}

function project(root, file, fields) {
  const test = basename(file, '.lua');
  // `features: *` is the whole app: the app project, which every feature and library hangs off
  const dependsOn =
    fields.features === '*'
      ? ['mla-tamagui']
      : list(fields.features).map((f) => featureProject(root, f));
  const targets = {};
  for (const platform of list(fields.platform || 'android')) {
    const inputs = [
      // the test case: its script and the data it reads
      { fileset: `{workspaceRoot}/${file}` },
      { fileset: `{workspaceRoot}/docs/runbooks/goldens/${platform}/**/*` },
      ...(platform === 'android'
        ? [{ fileset: '{workspaceRoot}/e2e/warm-app.lua' }]
        : []),
      ...[...HARNESS.shared, ...(HARNESS[platform] ?? [])].map((glob) => ({ fileset: `{workspaceRoot}/${glob}` })),
      // the feature code it drives and what that code imports: libraries, and npm packages at their locked (patched) version
      '^production',
      // the app shell every screen runs in: the app's own routes, layouts and components, not the features or libraries
      { input: 'production', projects: ['mla-tamagui'] },
      // only the npm packages the features import (above), not every package of the lockfile
      { externalDependencies: [] },
      // the browser build and platform the page renders on (the device an Android test drives is the VM's alone)
      ...(platform === 'browser' ? ['browser'] : []),
    ];
    const out = `.e2e-results/${platform}/${test}.json`;
    targets[`e2e-${platform}`] = {
      executor: 'nx:run-commands',
      cache: true,
      inputs,
      outputs: [`{workspaceRoot}/${out}`],
      options: {
        cwd: '{workspaceRoot}',
        // WF_E2E_PROBE (a file): name the test there instead of running it, so the job boots the device only when one will run
        command: [
          `if [ -n "$WF_E2E_PROBE" ]; then echo "${test}" >> "$WF_E2E_PROBE"; exit 3; fi`,
          // the wf that runs the stage (Nx puts node_modules/.bin, which has another wf, first on PATH)
          `"$WF_E2E_BIN" test run ${test} --platform ${platform} --sha "$WF_E2E_SHA" --out "$WF_E2E_OUT/${test}" -- $WF_E2E_FLAGS`,
          `mkdir -p .e2e-results/${platform}`,
          `cp "$WF_E2E_OUT/${test}/summary.json" ${out}`,
        ].join(' && '),
      },
    };
  }
  return {
    // no files live here: a test's files are named in its inputs, so one test's edit leaves the others cached
    root: `e2e/.nx/${test}`,
    name: `e2e-${test}`,
    tags: ['e2e'],
    // not the services it calls: their own deployment verification tests cover them (owner, 18 Oct)
    implicitDependencies: dependsOn,
    targets,
  };
}

// the manifests and feature project files it reads are matched too, so Nx re-runs it when one of them changes
exports.createNodes = [
  '{e2e/*.lua,frontend/mla-tamagui/features/*/{project,package}.json}',
  (files, _options, context) =>
    files
      .filter((file) => file.endsWith('.lua'))
      .flatMap((file) => {
        const fields = header(
          readFileSync(join(context.workspaceRoot, file), 'utf8')
        );
        if (!fields || !fields.platform) return [];
        const p = project(context.workspaceRoot, file, fields);
        return [[file, { projects: { [p.root]: p } }]];
      }),
];
