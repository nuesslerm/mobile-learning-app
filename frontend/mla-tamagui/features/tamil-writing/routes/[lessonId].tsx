import { useLocalSearchParams } from 'expo-router';

import { TamilLessonScreen } from '../ui/TamilLessonScreen';
import { tamilPageParams } from '../di/pageParams';

export const generateStaticParams = tamilPageParams.lessons;

export default function TamilLessonRoute() {
  const { lessonId } = useLocalSearchParams<{ lessonId: string }>();
  // A link to another lesson updates this screen's params in place; the key starts the new lesson at its first letter.
  return <TamilLessonScreen key={lessonId} lessonId={lessonId ?? ''} />;
}