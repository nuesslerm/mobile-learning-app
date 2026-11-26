import { Directory, File, Paths } from 'expo-file-system';
import { randomUUID } from 'expo-crypto';
import type { ChatImageFile, ImageActions } from '../domain/imageActions';

const extensions: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/heic': 'heic',
  'image/heif': 'heif',
  'image/avif': 'avif',
  'image/bmp': 'bmp',
};

export function imageActionsFor(image: ChatImageFile): ImageActions {
  const extension = extensions[image.mimeType.toLowerCase()];
  if (!extension) return {};
  async function withFile(perform: (file: File) => Promise<void>) {
    const folder = new Directory(Paths.cache, 'chat-image-' + randomUUID());
    folder.create();
    try {
      const file = await File.downloadFileAsync(
        image.uri,
        new File(folder, 'image.' + extension)
      );
      await perform(file);
    } finally {
      if (folder.exists) folder.delete();
    }
  }
  return {
    copy: () =>
      withFile(async (file) => {
        if (file.size > 18 * 1024 * 1024)
          throw new Error('Image exceeds the clipboard limit.');
        const { setImageAsync } = await import('expo-clipboard');
        await setImageAsync(await file.base64());
      }),
    save: () =>
      withFile(async (file) => {
        // The add-only API saves without requesting access to existing photos.
        const { saveToLibraryAsync } =
          await import('expo-media-library/legacy');
        await saveToLibraryAsync(file.uri);
      }),
    share: () =>
      withFile(async (file) => {
        const { shareAsync } = await import('expo-sharing');
        await shareAsync(file.uri, { mimeType: image.mimeType });
      }),
  };
}
