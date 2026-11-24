import { Logger } from '@lib/logging';
import { Platform, Share } from 'react-native';

export function shareProblem(id: string, title: string) {
  const url = `https://leetcode.com/problems/${encodeURIComponent(id)}/`;
  return Share.share(Platform.OS === 'ios' ? { url } : { message: url, title }).catch((error: unknown) => {
    Logger.withError(error).error('leetcode.library.share_failed');
  });
}
