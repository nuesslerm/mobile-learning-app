import { useLocalSearchParams } from 'expo-router';

import { tamilPageParams } from '../../../di/pageParams';
import { TamilTaskScreen } from '../../../ui/lessons/TamilTaskScreen';

export const generateStaticParams = tamilPageParams.tasks;

export default function TamilTaskRoute() {
  const { groupId, taskId } = useLocalSearchParams<{ groupId: string; taskId: string }>();
  return <TamilTaskScreen topicId={groupId} taskId={taskId} />;
}
