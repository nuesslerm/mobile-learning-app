import { useLocalSearchParams } from 'expo-router';

import { AlgorithmsLibraryScreen } from '../ui/library/AlgorithmsLibraryScreen';

export default function LibraryCollectionRoute() {
  const { collection, search } = useLocalSearchParams<{ collection?: string; search?: string }>();
  return <AlgorithmsLibraryScreen collection={collection ?? 'Algorithms'} focusSearch={search === '1'} />;
}
