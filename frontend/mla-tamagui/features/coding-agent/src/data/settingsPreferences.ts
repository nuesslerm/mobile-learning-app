// features/coding-agent/src/data/settingsPreferences.ts
//
// The settings this app keeps on the device, in AsyncStorage. No secrets: the
// values are a theme choice, a notification opt-in and the time of the last
// update check.

import AsyncStorage from '@react-native-async-storage/async-storage';
import { Logger } from '@lib/logging';

import type {
  AppearancePreference,
  SettingsPreferencesPort,
} from '../domain/ports';

export const CODING_AGENT_SETTINGS_STORAGE_KEY = 'coding-agent.settings';

const APPEARANCE_KEY = 'appearance';
const NOTIFICATIONS_KEY = 'notificationsEnabled';
const LAST_UPDATE_CHECK_KEY = 'lastUpdateCheckAt';

const APPEARANCES: readonly AppearancePreference[] = ['system', 'light', 'dark'];

type KeyValueStorage = Pick<typeof AsyncStorage, 'getItem' | 'setItem' | 'removeItem'>;

function appearance(value: string | null): AppearancePreference {
  return APPEARANCES.find((candidate) => candidate === value) ?? 'system';
}

/** The device's own settings, kept beside the coding agent's other state. */
export function createAsyncStorageSettingsPreferences(
  storage: KeyValueStorage = AsyncStorage
): SettingsPreferencesPort {
  const read = async (key: string): Promise<string | null> => {
    try {
      return await storage.getItem(`${CODING_AGENT_SETTINGS_STORAGE_KEY}.${key}`);
    } catch (cause) {
      Logger.withError(cause)
        .withMetadata({ key: `${CODING_AGENT_SETTINGS_STORAGE_KEY}.${key}` })
        .warn('coding-agent.settings.read_failed');
      return null;
    }
  };
  const write = async (key: string, value: string): Promise<void> => {
    try {
      await storage.setItem(`${CODING_AGENT_SETTINGS_STORAGE_KEY}.${key}`, value);
    } catch (cause) {
      Logger.withError(cause)
        .withMetadata({ key: `${CODING_AGENT_SETTINGS_STORAGE_KEY}.${key}` })
        .warn('coding-agent.settings.write_failed');
    }
  };
  return {
    async readChatText() {
      const raw = await read('chatText');
      if (raw === null) return null;
      try {
        const value: unknown = JSON.parse(raw);
        if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
        const { size, font, lineSpacing } = value as Record<string, unknown>;
        return {
          size: typeof size === 'number' && Number.isFinite(size) ? Math.min(24, Math.max(11, Math.round(size))) : undefined,
          font: font === 'system' || font === 'mono' || font === 'serif' ? font : undefined,
          lineSpacing: lineSpacing === 'default' || lineSpacing === 'compact' || lineSpacing === 'relaxed' ? lineSpacing : undefined,
        };
      } catch (cause) {
        Logger.withError(cause).warn('coding-agent.settings.invalid_chat_text');
        return null;
      }
    },
    async writeChatText(value) {
      const key = CODING_AGENT_SETTINGS_STORAGE_KEY + '.chatText';
      if (value === null) await storage.removeItem(key);
      else await storage.setItem(key, JSON.stringify(value));
    },
    async readAppearance() {
      return appearance(await read(APPEARANCE_KEY));
    },
    async writeAppearance(preference) {
      await write(APPEARANCE_KEY, preference);
    },
    async readNotificationsEnabled() {
      return (await read(NOTIFICATIONS_KEY)) === 'true';
    },
    async writeNotificationsEnabled(enabled) {
      await write(NOTIFICATIONS_KEY, enabled ? 'true' : 'false');
    },
    async readLastUpdateCheckAt() {
      const value = await read(LAST_UPDATE_CHECK_KEY);
      const at = value === null ? Number.NaN : Number(value);
      return Number.isFinite(at) ? at : null;
    },
    async writeLastUpdateCheckAt(at) {
      await write(LAST_UPDATE_CHECK_KEY, String(at));
    },
  };
}
