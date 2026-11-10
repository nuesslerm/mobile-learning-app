// Safe wrapper for expo-native-launch-arguments that degrades gracefully
// when the native module isn't available (e.g. Expo Go, web).
// The real package calls requireNativeModule('LaunchArguments') at module
// load time, which throws in environments where the native code isn't linked.

import { Logger } from '@lib/logging';
type ParsedMap = Record<string, string | boolean | number>;
let _LaunchArguments: {
  value<T extends object = ParsedMap>(): T;
} | null = null;
try {
  // eslint-disable-next-line @typescript-eslint/no-require-imports -- local escape; require() needed for platform-conditional load
  const mod = require('expo-native-launch-arguments');
  _LaunchArguments = mod.LaunchArguments;
} catch (err) {
  // Expected in Expo Go and on web (no native module), so not a warning; falls back to the stub below.
  Logger.withError(err).debug('launchArgs.nativeModule_unavailable');
}
export const LaunchArguments = _LaunchArguments ?? {
  value<T extends object = ParsedMap>(): T {
    // eslint-disable-next-line no-restricted-syntax -- local escape; switching to Zod/guards would not improve runtime safety here
    return {} as T;
  }
};