import { useEffect } from 'react';
import { useLocalSearchParams } from 'expo-router';

import { useThreadSettingsRequestStore } from '../state/threadSettingsRequestStore';
import { ThreadSettingsSheet } from './ThreadSettingsSheet';

export function ThreadSettingsOverlay() {
  const { threadId } = useLocalSearchParams<{ threadId: string }>();
  const request = useThreadSettingsRequestStore((state) => state.request);

  useEffect(() => {
    if (threadId !== undefined) request({ threadId });
  }, [threadId, request]);

  return <ThreadSettingsSheet />;
}
