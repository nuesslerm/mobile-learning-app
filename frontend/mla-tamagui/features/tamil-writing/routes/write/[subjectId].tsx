import { useLocalSearchParams } from 'expo-router';

import { TamilWriteScreen, tamilPageParams } from '@/features/tamil-writing';

export const generateStaticParams = tamilPageParams.subjects;

export default function TamilWriteRoute() {
  const { subjectId } = useLocalSearchParams<{ subjectId: string }>();
  // A link to another subject updates this screen's params in place; the key gives the new subject an empty pad.
  return <TamilWriteScreen key={subjectId} subjectId={subjectId} />;
}