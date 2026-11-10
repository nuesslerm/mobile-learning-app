import { useLocalSearchParams } from 'expo-router';

import { TamilReviewScreen } from '@/features/tamil-writing';

export default function TamilReviewRoute() {
  const { lessonId } = useLocalSearchParams<{ lessonId?: string }>();
  return <TamilReviewScreen {...(lessonId ? { lessonId } : {})} />;
}