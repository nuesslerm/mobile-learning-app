import type {
  AppearancePreference,
  AppInfoPort,
  AppUpdateCheck,
  AppUpdatePort,
  ExternalLinksPort,
  SettingsPreferencesPort,
} from '../domain/ports';

export interface SettingsPortDoubles {
  preferences: SettingsPreferencesPort;
  appUpdate: AppUpdatePort;
  appInfo: AppInfoPort;
  externalLinks: ExternalLinksPort;
  /** What the doubles were asked to do, so a test can assert it. */
  recorded: {
    appearance: AppearancePreference;
    notificationsEnabled: boolean;
    lastUpdateCheckAt: number | null;
    checks: number;
    installed: string[];
    opened: string[];
  };
}

/** The settings ports as in-memory doubles that record what they were asked to do. */
export function createSettingsPortDoubles(
  options: { update?: AppUpdateCheck } = {}
): SettingsPortDoubles {
  const recorded: SettingsPortDoubles['recorded'] = {
    appearance: 'system',
    notificationsEnabled: false,
    lastUpdateCheckAt: null,
    checks: 0,
    installed: [],
    opened: [],
  };
  const preferences: SettingsPreferencesPort = {
    readAppearance: async () => recorded.appearance,
    writeAppearance: async (preference) => {
      recorded.appearance = preference;
    },
    readNotificationsEnabled: async () => recorded.notificationsEnabled,
    writeNotificationsEnabled: async (enabled) => {
      recorded.notificationsEnabled = enabled;
    },
    readLastUpdateCheckAt: async () => recorded.lastUpdateCheckAt,
    writeLastUpdateCheckAt: async (at) => {
      recorded.lastUpdateCheckAt = at;
    },
  };
  const appUpdate: AppUpdatePort = {
    checkNow: async () => {
      recorded.checks += 1;
      return options.update ?? { status: 'up-to-date' };
    },
    install: async (updateId) => {
      recorded.installed.push(updateId);
    },
  };
  const appInfo: AppInfoPort = {
    version: async () => '1.0.0',
    build: async () => '1',
  };
  const externalLinks: ExternalLinksPort = {
    open: async (url) => {
      recorded.opened.push(url);
    },
  };
  return { preferences, appUpdate, appInfo, externalLinks, recorded };
}
