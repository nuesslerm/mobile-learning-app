import { Button, Host, HStack, Image, Menu, Section, Text, Toggle } from '@expo/ui/swift-ui';
import {
  accessibilityIdentifier,
  accessibilityLabel,
  buttonStyle,
  disabled,
  fixedSize,
  font,
  foregroundStyle,
  frame,
  glassEffect,
  menuActionDismissBehavior,
  menuOrder,
  menuStyle,
  padding,
} from '@expo/ui/swift-ui/modifiers';
import { isGlassEffectAPIAvailable, isLiquidGlassAvailable } from 'expo-glass-effect';
import type { ReactElement } from 'react';

import type { GlassMenuGroupRenderer } from './glass-menu-group';
import type { MenuNode } from './header-menu-spec';

function menuNode(node: MenuNode): ReactElement {
  const id = accessibilityIdentifier(node.testID);
  switch (node.kind) {
    case 'section':
      return <Section key={node.testID} title={node.title}>{node.items.map(menuNode)}</Section>;
    case 'submenu':
      return <Menu key={node.testID} label={node.label} systemImage={node.sfSymbol} modifiers={[id]}>
        {node.items.map(menuNode)}
      </Menu>;
    case 'confirm':
      return <Menu key={node.testID} label={node.label} systemImage={node.sfSymbol} modifiers={[id]}>
        <Button label={node.confirmLabel} role="destructive" onPress={node.onConfirm} />
      </Menu>;
    default: {
      const modifiers = [id, disabled(node.disabled ?? false), menuActionDismissBehavior(node.keepOpen ? 'disabled' : 'enabled')];
      if (node.checked !== undefined) {
        return <Toggle key={node.testID} label={node.label} systemImage={node.sfSymbol} isOn={node.checked} onIsOnChange={node.onPress} modifiers={modifiers} />;
      }
      return <Button key={node.testID} label={node.label} systemImage={node.sfSymbol} role={node.destructive ? 'destructive' : undefined} onPress={node.onPress} modifiers={modifiers} />;
    }
  }
}

export const renderGlassMenuGroup: GlassMenuGroupRenderer = (menus, scheme) => {
  if (!isLiquidGlassAvailable() || !isGlassEffectAPIAvailable()) return undefined;

  return (
    <Host matchContents alignment="trailing" colorScheme={scheme} ignoreSafeArea="all">
      <HStack spacing={0} modifiers={[padding({ horizontal: 5 }), glassEffect({ shape: 'capsule' })]}>
        {menus.map((menu) => (
          <Menu
            key={menu.testID}
            modifiers={[
              menuStyle('button'),
              buttonStyle('plain'),
              menuOrder('fixed'),
              accessibilityLabel(menu.accessibilityLabel),
              accessibilityIdentifier(menu.testID),
            ]}
            label={menu.label || !menu.sfSymbol ? (
              <Text modifiers={[font({ size: 16 }), foregroundStyle('primary'), fixedSize(), padding({ horizontal: 10 }), frame({ minWidth: 76, minHeight: 44, maxHeight: 44 })]}>
                {menu.label ?? menu.accessibilityLabel}
              </Text>
            ) : (
              <Image systemName={menu.sfSymbol} size={17} modifiers={[foregroundStyle('primary'), frame({ width: 40, height: 44 })]} />
            )}
          >
            {menu.items.map(menuNode)}
          </Menu>
        ))}
      </HStack>
    </Host>
  );
};
