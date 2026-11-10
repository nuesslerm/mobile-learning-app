import { useLocalSearchParams } from 'expo-router';

import { ResetCodeDialog } from '../../ui/ResetCodeDialog';
import { dismissOverlayRoute } from '../../ui/dismissOverlayRoute';

export default function LeetCodeResetCodeRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return (
    <ResetCodeDialog
      onDismiss={() => {
        dismissOverlayRoute(id);
      }}
    />
  );
}