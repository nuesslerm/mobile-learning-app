import { useLocalSearchParams } from 'expo-router';

import { TamilLessonScreen, tamilPageParams } from '@/features/tamil-writing';

export const generateStaticParams = tamilPageParams.lessons;

export default function TamilLessonRoute() {
  const { lessonId } = useLocalSearchParams<{ lessonId: string }>();
  // A link to another lesson updates this screen's params in place; the key starts the new lesson at its first letter.
  return <TamilLessonScreen key={lessonId} lessonId={lessonId ?? ''} />;
}