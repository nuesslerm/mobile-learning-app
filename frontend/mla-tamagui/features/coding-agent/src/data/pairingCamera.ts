import { Camera } from 'expo-camera';
import type { PairingCameraPort } from '../domain/ports';

export const pairingCamera: PairingCameraPort = {
  get: Camera.getCameraPermissionsAsync,
  request: Camera.requestCameraPermissionsAsync,
};
