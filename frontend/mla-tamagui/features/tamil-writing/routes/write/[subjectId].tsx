import { useLocalSearchParams } from 'expo-router';

import { TamilWriteScreen } from '../../ui/TamilWriteScreen';
import { tamilPageParams } from '../../di/pageParams';

export const generateStaticParams = tamilPageParams.subjects;

export default function TamilWriteRoute() {
  const { subjectId } = useLocalSearchParams<{ subjectId: string }>();
  // A link to another subject updates this screen's params in place; the key gives the new subject an empty pad.
  return <TamilWriteScreen key={subjectId} subjectId={subjectId} />;
}