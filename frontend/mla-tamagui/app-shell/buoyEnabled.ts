import { Platform } from 'react-native';

// A device-lab build is driven through the accessibility tree, and Buoy's overlay leaves only its own button in it on iOS.
export const buoyEnabled =
  Platform.OS !== 'web' &&
  process.env.EXPO_PUBLIC_DEVICE_LAB !== '1' &&
  (__DEV__ || process.env.EXPO_PUBLIC_APP_ENV === 'staging');
