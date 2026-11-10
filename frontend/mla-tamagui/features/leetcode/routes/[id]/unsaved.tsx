import { useLocalSearchParams } from 'expo-router';

import { UnsavedChangesDialog } from '../../ui/UnsavedChangesDialog';
import { dismissOverlayRoute } from '../../ui/dismissOverlayRoute';

export default function LeetCodeUnsavedChangesRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return (
    <UnsavedChangesDialog
      onDismiss={() => {
        dismissOverlayRoute(id);
      }}
    />
  );
}