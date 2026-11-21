import { useLocalSearchParams } from 'expo-router';

import { TamilReviewScreen } from '../ui/TamilReviewScreen';

export default function TamilReviewRoute() {
  const { lessonId } = useLocalSearchParams<{ lessonId?: string }>();
  return <TamilReviewScreen {...(lessonId ? { lessonId } : {})} />;
}