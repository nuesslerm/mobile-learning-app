import type { LibraryCoverPort } from '../../domain/library/library';

export const deviceLibraryCover: LibraryCoverPort = {
  async pick() {
    const { File, Paths } = await import('expo-file-system');
    const picked = await File.pickFileAsync({ mimeTypes: 'image/*' });
    if (picked.canceled) return null;
    const cover = new File(Paths.document, `leetcode-cover-${Date.now()}${picked.result.extension}`);
    await picked.result.copy(cover);
    return cover.uri;
  },
};
