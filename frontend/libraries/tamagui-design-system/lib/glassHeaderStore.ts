import AsyncStorage from '@react-native-async-storage/async-storage';

import { createGlassHeaderStore } from './createGlassHeaderStore';

export { DEFAULT_GLASS_HEADERS, GLASS_HEADERS_STORAGE_KEY } from './createGlassHeaderStore';

// AsyncStorage directly, not lib/keyValueStore: that adapter is an in-memory map on native, so the setting would reset at every update load on a phone.
/** Whether a screen that opted in may show the native glass top bar (iOS 26 and later); kept across launches. */
export const useGlassHeaderStore = createGlassHeaderStore(AsyncStorage);
