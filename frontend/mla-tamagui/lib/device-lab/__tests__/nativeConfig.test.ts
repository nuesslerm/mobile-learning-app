import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

/**
 * Expo Go supplies every native permission string and config plugin itself, so a missing one never fails there and
 * only a dev or EAS build finds it (device-lab spec J5).
 */
const require = createRequire(import.meta.url);
const appDir = path.resolve(__dirname, '../../..');
const repo = path.resolve(appDir, '../..');
const pkg = JSON.parse(fs.readFileSync(path.join(appDir, 'package.json'), 'utf8')) as { dependencies: Record<string, string> };
const config = (require('../../../app.config.js') as { expo?: Record<string, unknown> } | (() => { expo: Record<string, unknown> }));
const expo = (typeof config === 'function' ? config() : config).expo as { plugins?: Array<string | [string, unknown]>; runtimeVersion?: string };

/** Dependencies that ship a config plugin this app deliberately does not list, and why. */
const UNLISTED = new Map<string, string>([
  ['expo-font', 'no `fonts` are embedded natively: the plugin only embeds them'],
  ['expo-system-ui', 'no plugin options in use; userInterfaceStyle is a top-level key'],
  ['expo-updates', 'configured through the top-level `updates` and `runtimeVersion` keys'],
]);

const listed = new Set((expo.plugins ?? []).map((plugin) => (Array.isArray(plugin) ? plugin[0] : plugin)));

function shipsPlugin(dependency: string): boolean {
  return fs.existsSync(path.join(appDir, 'node_modules', dependency, 'app.plugin.js'));
}

describe('native config', () => {
  it('Given every dependency that ships a config plugin, When the app config is read, Then the plugin is listed or its absence is explained', () => {
    // Given the dependencies with an app.plugin.js
    const withPlugin = Object.keys(pkg.dependencies).filter(shipsPlugin);

    // When compared with the plugins the config lists and the explained omissions
    const unexplained = withPlugin.filter((dependency) => !listed.has(dependency) && !UNLISTED.has(dependency));

    // Then none is silently missing: expo-audio's microphone string in particular
    expect(unexplained).toEqual([]);
    expect(listed.has('expo-audio')).toBe(true);
  });

  it('Given the microphone plugin, When configured, Then it carries a permission string', () => {
    // Given the expo-audio entry
    const entry = (expo.plugins ?? []).find((plugin) => Array.isArray(plugin) && plugin[0] === 'expo-audio') as [string, { microphonePermission?: string }] | undefined;

    // Then iOS gets its NSMicrophoneUsageDescription
    expect(entry?.[1].microphonePermission?.length).toBeGreaterThan(10);
  });

  it('Given the runtime version, When compared with the documented Expo Go client, Then they agree', () => {
    // Given the version the publishing guide says Expo Go loads updates for
    const guide = fs.readFileSync(path.join(repo, 'docs/solutions/workflow/eas-publishing.md'), 'utf8');
    const documented = /Runtime version: `(exposdk:[\d.]+)`/.exec(guide)?.[1];

    // Then the config publishes for that client. It is the Expo Go client's SDK, not the `expo` package's: updates
    // are loadable only when the two agree, so changing one means changing the other and re-checking Expo Go.
    expect(documented).toBeDefined();
    expect(expo.runtimeVersion).toBe(documented);
  });
});
