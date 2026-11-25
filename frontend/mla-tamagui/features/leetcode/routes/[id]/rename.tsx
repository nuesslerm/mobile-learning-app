import { useLocalSearchParams } from 'expo-router';

import { RenameSymbolDialog } from '../../ui/RenameSymbolDialog';
import { useDismissOverlayRoute } from '../../ui/dismissOverlayRoute';

export default function LeetCodeRenameSymbolRoute() {
  const { id, symbol } = useLocalSearchParams<{ id: string; symbol?: string }>();
  const dismiss = useDismissOverlayRoute(id);

  return (
    <RenameSymbolDialog
      initialTarget={symbol}
      onDismiss={dismiss}
    />
  );
}