import { useLocalSearchParams } from 'expo-router';

import { TamilTrackScreen, tamilPageParams } from '@/features/tamil-writing';

export const generateStaticParams = tamilPageParams.tracks;

export default function TamilTrackRoute() {
  const { trackId } = useLocalSearchParams<{ trackId: string }>();
  return <TamilTrackScreen trackId={trackId} />;
}