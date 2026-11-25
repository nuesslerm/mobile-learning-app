import type { ReactNode } from 'react';
import { Modal, type ViewStyle } from 'react-native';

import { YStack } from '../primitives';

export type PageSheetProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDismiss?: () => void;
  testID: string;
  children: ReactNode;
  contentStyle?: ViewStyle;
};

export function PageSheet({ open, onOpenChange, onDismiss, testID, children, contentStyle }: PageSheetProps) {
  return (
    <Modal
      visible={open}
      animationType="slide"
      presentationStyle="pageSheet"
      allowSwipeDismissal
      onRequestClose={() => onOpenChange(false)}
      onDismiss={onDismiss}
    >
      <YStack
        testID={testID}
        flex={1}
        bg="$color2"
        px={contentStyle?.paddingHorizontal === undefined ? '$4' : undefined}
        pt={contentStyle?.paddingTop === undefined ? '$3' : undefined}
        style={contentStyle}
      >
        {children}
      </YStack>
    </Modal>
  );
}
