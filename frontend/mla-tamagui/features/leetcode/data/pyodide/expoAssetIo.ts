import type { AssetIo } from './pyodideAssets';

const CHUNK_BYTES = 393_216;

function base64Of(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 8192)
    binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return btoa(binary);
}

/** The only place expo-asset and expo-file-system are used for the runner; both load when a run first needs them, not with the app. */
export function createExpoAssetIo(): AssetIo {
  return {
    async resolve(moduleId) {
      const { Asset } = await import('expo-asset');
      const loaded = await Asset.fromModule(moduleId).downloadAsync();
      return loaded.localUri ?? loaded.uri;
    },
    async writePage(html) {
      const { File, Paths } = await import('expo-file-system');
      const file = new File(Paths.cache, 'pyodide-page.html');
      if (!file.exists) file.create();
      file.writeSync(html);
      return file.uri;
    },
    async *chunks(uri) {
      const { File, FileMode } = await import('expo-file-system');
      const handle = new File(uri).open(FileMode.ReadOnly);
      try {
        const total = Math.max(1, Math.ceil((handle.size ?? 0) / CHUNK_BYTES));
        for (let index = 0; index < total; index += 1)
          yield {
            index,
            total,
            data: base64Of(await handle.readBytes(CHUNK_BYTES)),
          };
      } finally {
        handle.close();
      }
    },
  };
}
