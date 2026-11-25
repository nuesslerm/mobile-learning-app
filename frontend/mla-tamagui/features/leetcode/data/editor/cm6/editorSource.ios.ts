import { Logger } from '@lib/logging';
import { File, Paths } from 'expo-file-system';

let cached: File | undefined;

export function editorSource(html: string): { html: string } | { uri: string } {
  try {
    if (!cached?.exists) {
      const page = new File(Paths.cache, 'leetcode-editor.html');
      if (!page.exists) page.create();
      page.writeSync(html);
      cached = page;
    }
    return { uri: cached.uri };
  } catch (error) {
    Logger.withError(error).warn('leetcode.editor.page_cache_failed');
    return { html };
  }
}
