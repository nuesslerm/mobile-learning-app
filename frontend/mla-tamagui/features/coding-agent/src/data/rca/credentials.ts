import { Platform } from 'react-native';
import * as SecureStore from 'expo-secure-store';
import type { Pairing } from '../../domain/mobile';

const key = 'rca.mobile.connection.v1';
export async function readPairing(): Promise<Pairing | null> {
  const raw = Platform.OS === 'web' ? globalThis.sessionStorage?.getItem(key) : await SecureStore.getItemAsync(key);
  if (!raw) return null;
  const saved: Pairing = JSON.parse(raw);
  if (typeof saved.endpoint !== 'string' || typeof saved.token !== 'string') throw new Error('Invalid saved pairing');
  return saved;
}
export async function savePairing(pair: Pairing | null) {
  if (Platform.OS === 'web') {
    if (pair) globalThis.sessionStorage.setItem(key, JSON.stringify(pair));
    else globalThis.sessionStorage.removeItem(key);
  } else if (pair) {
    await SecureStore.setItemAsync(key, JSON.stringify(pair), { keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY });
  } else {
    await SecureStore.deleteItemAsync(key);
  }
}
