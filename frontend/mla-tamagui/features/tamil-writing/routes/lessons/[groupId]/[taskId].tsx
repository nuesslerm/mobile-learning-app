import { useLocalSearchParams } from 'expo-router';

import { tamilPageParams } from '../../../di/pageParams';
import { TamilTaskScreen } from '../../../ui/lessons/TamilTaskScreen';

export const generateStaticParams = tamilPageParams.tasks;

export default function TamilTaskRoute() {
  const { groupId, taskId, mode } = useLocalSearchParams<{ groupId: string; taskId: string; mode?: string }>();
  return <TamilTaskScreen topicId={groupId} taskId={taskId} flashCards={mode === 'flash-cards'} />;
}
