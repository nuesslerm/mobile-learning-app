import { PanZoomImage, useImageActions, type ChatImageViewerProps } from '@lib/skin';
import { Icon, IconButton, InsetGroup, ListItem, Text } from '@lib/tamagui-design-system';
import { Copy, Download, Share, X } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { useState } from 'react';
import { Modal, StyleSheet } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';

const ACTION_ICONS = { copy: Copy, save: Download, share: Share } as const;

/** An image full size over the whole window: pan and zoom, a close button, and what can be done with it in the app's sheet. */
export function ChatImageViewer({ uri, label, topInset, rightInset, bottomInset = 0, actions, onClose }: ChatImageViewerProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const { busy, message, offered, run } = useImageActions(actions, setMenuOpen);
  return (
    <Modal visible transparent animationType="fade" onRequestClose={() => (menuOpen ? setMenuOpen(false) : onClose())}>
      {/* a Modal is a window of its own, and Android delivers gestures only under a root view inside it */}
      <GestureHandlerRootView style={StyleSheet.absoluteFill}>
        <YStack flex={1} bg="$background">
          <PanZoomImage uri={uri} label={label} />
          <XStack position="absolute" t={topInset + 8} r={rightInset + 16} gap="$2">
            {offered.length > 0 ? (
              <IconButton
                icon={Share}
                variant="secondary"
                accessibilityLabel="Image actions"
                disabled={busy || menuOpen}
                pressLog="off"
                onPress={() => setMenuOpen(true)}
                testID="skin-image-actions"
              />
            ) : null}
            <IconButton icon={X} variant="secondary" accessibilityLabel="Close image" pressLog="off" onPress={onClose} testID="skin-image-close" />
          </XStack>
          {message ? (
            <XStack position="absolute" l={0} r={0} b={bottomInset + 20} justify="center" pointerEvents="none">
              <YStack bg="$color12" rounded="$10" px="$4" py="$2">
                <Text role="alert" color="$color1">
                  {message}
                </Text>
              </YStack>
            </XStack>
          ) : null}
          <Sheet open={menuOpen} onOpenChange={setMenuOpen} snapPoints={[35]} handleLabel="Dismiss image actions" testID="skin-image-actions-sheet">
            <YStack pb={bottomInset + 16}>
              <InsetGroup testID="skin-image-actions-list">
                {offered.map((row) => (
                  <ListItem key={row.id} variant="grouped" pressLog="off" accessibilityLabel={row.label} onPress={() => run(row)} testID={`skin-image-action-${row.id}`}>
                    <XStack items="center" gap="$3">
                      <Icon as={ACTION_ICONS[row.id]} size={16} color="$color11" />
                      <Text>{row.label}</Text>
                    </XStack>
                  </ListItem>
                ))}
              </InsetGroup>
            </YStack>
          </Sheet>
        </YStack>
      </GestureHandlerRootView>
    </Modal>
  );
}
