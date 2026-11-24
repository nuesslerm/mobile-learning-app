import { Text, useOpenSheetStore } from '@lib/tamagui-design-system';
import { YStack } from '@lib/tamagui-design-system/primitives';
import React from 'react';
import { Pressable, StyleSheet } from 'react-native';

import { ScanGlyph } from './ScanGlyph';

/**
 * Shown from the press of Run or Submit until the answer lands: a scrim that swallows touches (a deliberate, transient exception to
 * the rule that nothing claims touches above the editor), and a translucent card with the scan glyph. While it is up the keyboard rail is gone.
 */
export function PendingOverlay() {
  React.useEffect(() => {
    const { open, close } = useOpenSheetStore.getState();
    open();
    return close;
  }, []);
  return (
    <Pressable
      accessible
      role="progressbar"
      aria-busy
      aria-label="Running your code"
      testID="ConsolePending"
      style={[StyleSheet.absoluteFill, { zIndex: 3000, alignItems: 'center', justifyContent: 'center' }]}
    >
      <YStack position="absolute" t={0} b={0} l={0} r={0} bg="$background" opacity={0.55} pointerEvents="none" />
      <YStack width={160} height={140} items="center" justify="center" gap="$3" rounded="$5" borderWidth={1} borderColor="$borderColor" bg="$color2" style={{ opacity: 0.92 }}>
        <ScanGlyph />
        <Text variant="strong" color="$warning" testID="ConsolePendingText">
          Pending...
        </Text>
      </YStack>
    </Pressable>
  );
}
