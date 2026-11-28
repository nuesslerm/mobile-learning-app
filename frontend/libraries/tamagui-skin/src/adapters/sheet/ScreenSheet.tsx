import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import type { ReactNode } from 'react';
import { Modal, Platform, StyleSheet, View, type ViewStyle } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';

type Props = {
  onClose: () => void;
  children: ReactNode;
  snapPoints?: number[];
  handleLabel: string;
  testID: string;
};

const WEB_WINDOW = { position: 'fixed', top: 0, right: 0, bottom: 0, left: 0, zIndex: 100, pointerEvents: 'box-none' } as unknown as ViewStyle;

/**
 * The app's sheet over the whole window. The design system's sheet is inline: it covers only the view it is mounted in, and the
 * skin's sheets open from inside a tab or under a navigator's bar.
 */
export function ScreenSheet({ onClose, children, snapPoints = [45], handleLabel, testID }: Props) {
  const sheet = (
    <Sheet open onOpenChange={(open) => !open && onClose()} snapPoints={snapPoints} handleLabel={handleLabel} testID={testID}>
      {children}
    </Sheet>
  );
  // the web's Modal renders outside the themed subtree and off the keyboard's frame, so there the sheet stays inline, in a layer over the window
  if (Platform.OS === 'web') return <View style={WEB_WINDOW}>{sheet}</View>;
  return (
    <Modal visible transparent animationType="none" onRequestClose={onClose}>
      {/* a Modal is a window of its own, and Android delivers gestures only under a root view inside it */}
      <GestureHandlerRootView style={StyleSheet.absoluteFill}>{sheet}</GestureHandlerRootView>
    </Modal>
  );
}
