import { launchImageLibraryAsync } from 'expo-image-picker';
import { randomUUID } from 'expo-crypto';
import { Platform } from 'react-native';

const maxEncoded = Math.ceil((10 * 1024 * 1024) / 3) * 4;

type PreviewWindow = Window & {
  rcaImagePickerUntil?: number;
  rcaImageRequest?: {
    id: string;
    receive: (part: string | null, done: boolean) => void;
  };
};

export async function pickTerminalImage(): Promise<string | null> {
  if (Platform.OS === 'web') {
    const page = window as PreviewWindow;
    if ((page.rcaImagePickerUntil ?? 0) >= Date.now()) {
      delete page.rcaImagePickerUntil;
      return new Promise((resolve, reject) => {
        const parts: string[] = [];
        let length = 0;
        const timeout = setTimeout(() => receive(null, true), 300000);
        function receive(part: string | null, done: boolean) {
          length += part?.length ?? 0;
          if (part !== null && length <= maxEncoded) parts.push(part);
          if (!done && length <= maxEncoded) return;
          clearTimeout(timeout);
          delete page.rcaImageRequest;
          if (length > maxEncoded)
            reject(new Error('Choose an image smaller than 10 MB.'));
          else resolve(part === null ? null : parts.join(''));
        }
        page.rcaImageRequest = { id: randomUUID(), receive };
      });
    }
  }
  const result = await launchImageLibraryAsync({
    mediaTypes: ['images'],
    base64: true,
    quality: 0.8,
  });
  if (result.canceled) return null;
  const data = result.assets[0]?.base64;
  if (!data || data.length > maxEncoded)
    throw new Error('Choose an image smaller than 10 MB.');
  return data;
}
