// features/coding-agent/src/data/appUpdate.ts
//
// The app's own update channel, through Expo's update service. A check runs
// only when the user asks for one; nothing is fetched or applied on its own.

import { Logger } from '@lib/logging';

import type { AppUpdateCheck, AppUpdatePort } from '../domain/ports';

export function createExpoUpdatesAppUpdate(): AppUpdatePort {
  return {
    async checkNow(): Promise<AppUpdateCheck> {
      try {
        const Updates = await import('expo-updates');
        if (!Updates.isEnabled) {
          return { status: 'failed', message: 'This build cannot check for updates.' };
        }
        const result = await Updates.checkForUpdateAsync();
        if (!result.isAvailable) {
          return { status: 'up-to-date' };
        }
        return { status: 'available', updateId: result.manifest.id };
      } catch (cause) {
        Logger.withError(cause).warn('coding-agent.settings.update_check_failed');
        return { status: 'failed', message: 'Could not reach the update service.' };
      }
    },
    async install(updateId: string): Promise<void> {
      const Updates = await import('expo-updates');
      const fetched = await Updates.fetchUpdateAsync();
      if (!('isNew' in fetched) || !fetched.isNew) {
        throw new Error('The update service did not return a new update.');
      }
      Logger.info('coding-agent.settings.update_installed', {
        updateId,
        fetchedUpdateId: fetched.manifest.id,
      });
      await Updates.reloadAsync();
    },
  };
}
