import { useLocalSearchParams } from 'expo-router';

import { ProblemDetailScreen } from '../../ui/library/ProblemDetailScreen';

export default function ProblemDetailRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <ProblemDetailScreen key={id} id={id} />;
}
