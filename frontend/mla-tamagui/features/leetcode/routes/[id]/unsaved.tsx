import { useLocalSearchParams } from 'expo-router';

import { UnsavedChangesDialog } from '../../ui/UnsavedChangesDialog';
import { useDismissOverlayRoute } from '../../ui/dismissOverlayRoute';

export default function LeetCodeUnsavedChangesRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const dismiss = useDismissOverlayRoute(id);
  return (
    <UnsavedChangesDialog
      onDismiss={dismiss}
    />
  );
}