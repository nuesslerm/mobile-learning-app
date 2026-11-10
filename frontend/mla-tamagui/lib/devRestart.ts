import { Logger } from '@lib/logging';
import * as Updates from 'expo-updates';
import { DevSettings } from 'react-native';

export async function restart(): Promise<void> {
  const devReload = __DEV__ ? DevSettings?.reload : undefined;
  if (typeof devReload === 'function') {
    Logger.info('dev_restart.devsettings_reload', {
      mode: 'devsettings'
    });
    devReload();
    return;
  }
  try {
    await Updates.reloadAsync();
  } catch (err) {
    Logger.withError(err).warn('dev_restart.reload_failed');
  }
}
