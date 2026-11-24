import AsyncStorage from '@react-native-async-storage/async-storage';

import { createNativeHeaderStore } from './createNativeHeaderStore';

export { DEFAULT_NATIVE_HEADERS, NATIVE_HEADERS_STORAGE_KEY } from './createNativeHeaderStore';

// AsyncStorage directly, not lib/keyValueStore: that adapter is an in-memory map on native, so the setting would reset at every update load on a phone.
/** Whether a screen that opted in draws the iOS system header; on by default, and the off switch keeps the app's own header. */
export const useNativeHeaderStore = createNativeHeaderStore(AsyncStorage);
