import { useLocalSearchParams } from 'expo-router';

import { RenameSymbolDialog } from '../../ui/RenameSymbolDialog';
import { dismissOverlayRoute } from '../../ui/dismissOverlayRoute';

export default function LeetCodeRenameSymbolRoute() {
  const { id, symbol } = useLocalSearchParams<{ id: string; symbol?: string }>();

  return (
    <RenameSymbolDialog
      initialTarget={symbol}
      onDismiss={() => {
        dismissOverlayRoute(id);
      }}
    />
  );
}