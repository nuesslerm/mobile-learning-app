import { useLocalSearchParams } from 'expo-router';

import { tamilPageParams } from '../../../di/pageParams';
import { TamilTopicScreen } from '../../../ui/lessons/TamilTopicScreen';

export const generateStaticParams = tamilPageParams.topics;

export default function TamilTopicRoute() {
  const { groupId } = useLocalSearchParams<{ groupId: string }>();
  return <TamilTopicScreen topicId={groupId} />;
}
