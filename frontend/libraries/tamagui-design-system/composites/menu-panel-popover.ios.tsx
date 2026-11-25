import { Host, Popover, Rectangle, RNHostView } from '@expo/ui/swift-ui';
import { foregroundStyle } from '@expo/ui/swift-ui/modifiers';
import { isGlassEffectAPIAvailable, isLiquidGlassAvailable } from 'expo-glass-effect';
import { StyleSheet, View } from 'react-native';

import type { MenuPanelPopoverProps } from './menu-panel-popover';

export function supportsMenuPanelPopover(): boolean {
  return isLiquidGlassAvailable() && isGlassEffectAPIAvailable();
}

export function MenuPanelPopover({ trigger, content, width, open, placement, onClose }: MenuPanelPopoverProps) {
  const beside = placement === 'beside';
  return (
    <View>
      {trigger}
      <Host style={StyleSheet.absoluteFill} pointerEvents="none">
        <Popover
          attachmentAnchor={beside ? 'trailing' : 'top'}
          arrowEdge={beside ? 'leading' : 'bottom'}
          isPresented={open}
          onIsPresentedChange={(presented) => { if (!presented) onClose(); }}
        >
          <Popover.Trigger>
            <Rectangle modifiers={[foregroundStyle({ type: 'color', color: 'transparent' })]} />
          </Popover.Trigger>
          <Popover.Content>
            <RNHostView matchContents>
              <View style={{ width }}>{content}</View>
            </RNHostView>
          </Popover.Content>
        </Popover>
      </Host>
    </View>
  );
}
