import { useLocalSearchParams } from 'expo-router';

import { TamilTopicScreen } from '../ui/lessons/TamilTopicScreen';

export default function TamilAllTopicsRoute() {
  const { section } = useLocalSearchParams<{ section?: string }>();
  return <TamilTopicScreen sectionId={section} />;
}
