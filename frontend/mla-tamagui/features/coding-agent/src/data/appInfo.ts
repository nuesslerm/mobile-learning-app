// features/coding-agent/src/data/appInfo.ts
//
// The app's own identity, read from the build's config. The module is imported
// lazily so a test never loads the native side.

import type { AppInfoPort } from '../domain/ports';

export function createExpoConstantsAppInfo(): AppInfoPort {
  const read = async () => {
    const Constants = (await import('expo-constants')).default;
    return Constants.expoConfig ?? null;
  };
  return {
    async version() {
      return (await read())?.version ?? null;
    },
    async build() {
      const config = await read();
      const build = config?.ios?.buildNumber ?? config?.android?.versionCode;
      return build === undefined || build === null ? null : String(build);
    },
  };
}
