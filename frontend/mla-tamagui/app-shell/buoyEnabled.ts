import { Platform } from 'react-native';

export const buoyEnabled =
  Platform.OS !== 'web' && (__DEV__ || process.env.EXPO_PUBLIC_APP_ENV === 'staging');
