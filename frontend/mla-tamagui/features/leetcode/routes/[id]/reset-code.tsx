import { useLocalSearchParams } from 'expo-router';

import { ResetCodeDialog } from '../../ui/ResetCodeDialog';
import { useDismissOverlayRoute } from '../../ui/dismissOverlayRoute';

export default function LeetCodeResetCodeRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const dismiss = useDismissOverlayRoute(id);
  return (
    <ResetCodeDialog
      onDismiss={dismiss}
    />
  );
}