import AsyncStorage from '@react-native-async-storage/async-storage';

import type { SnapshotStorage } from './snapshotStorage';

export function createDeviceStorage(): SnapshotStorage {
  return AsyncStorage;
}
