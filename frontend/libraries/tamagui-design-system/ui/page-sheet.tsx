import type { ReactNode } from 'react';
import { Modal } from 'react-native';

import { YStack } from '../primitives';

export type PageSheetProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDismiss?: () => void;
  testID: string;
  children: ReactNode;
};

export function PageSheet({ open, onOpenChange, onDismiss, testID, children }: PageSheetProps) {
  return (
    <Modal
      visible={open}
      animationType="slide"
      presentationStyle="pageSheet"
      allowSwipeDismissal
      onRequestClose={() => onOpenChange(false)}
      onDismiss={onDismiss}
    >
      <YStack testID={testID} flex={1} bg="$color2" px="$4" pt="$3">
        {children}
      </YStack>
    </Modal>
  );
}
