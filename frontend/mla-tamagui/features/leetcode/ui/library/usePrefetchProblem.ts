import { router, useFocusEffect } from 'expo-router';
import { useCallback } from 'react';
import { Platform } from 'react-native';

export function usePrefetchProblem(id: string | undefined) {
  useFocusEffect(useCallback(() => {
    if (Platform.OS !== 'ios' || !id) return;
    const work = requestIdleCallback(() => router.prefetch(`/leetcode/${id}/problem`));
    return () => cancelIdleCallback(work);
  }, [id]));
}
