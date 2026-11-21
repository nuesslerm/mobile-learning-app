import { useLocalSearchParams } from 'expo-router';

import { TamilTrackScreen } from '../../ui/TamilTrackScreen';
import { tamilPageParams } from '../../di/pageParams';

export const generateStaticParams = tamilPageParams.tracks;

export default function TamilTrackRoute() {
  const { trackId } = useLocalSearchParams<{ trackId: string }>();
  return <TamilTrackScreen trackId={trackId} />;
}