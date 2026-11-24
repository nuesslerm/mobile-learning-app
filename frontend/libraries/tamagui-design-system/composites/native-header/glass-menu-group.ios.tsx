import { Alert, Button, Host, HStack, Image, Menu, Section, Text, Toggle } from '@expo/ui/swift-ui';
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
import { useState, type ReactElement } from 'react';

import { DARK_GLASS_CONTROL_TINT } from '../../ui/glass-control-background';

import type { GlassMenuGroupRenderer } from './glass-menu-group';
import type { HeaderMenuSpec, MenuConfirmNode, MenuNode } from './header-menu-spec';

function menuNode(node: MenuNode, confirm: (node: MenuConfirmNode) => void): ReactElement {
  const id = accessibilityIdentifier(node.testID);
  switch (node.kind) {
    case 'section':
      return <Section key={node.testID} title={node.title}>{node.items.map((item) => menuNode(item, confirm))}</Section>;
    case 'submenu':
      return <Menu key={node.testID} label={node.label} systemImage={node.sfSymbol} modifiers={[id]}>
        {node.items.map((item) => menuNode(item, confirm))}
      </Menu>;
    case 'confirm':
      return (
        <Button
          key={node.testID}
          label={node.label}
          systemImage={node.sfSymbol}
          modifiers={[id]}
          onPress={() => confirm(node)}
        />
      );
    default: {
      const modifiers = [id, disabled(node.disabled ?? false), menuActionDismissBehavior(node.keepOpen ? 'disabled' : 'enabled')];
      if (node.checked !== undefined) {
        return <Toggle key={node.testID} label={node.label} systemImage={node.sfSymbol} isOn={node.checked} onIsOnChange={node.onPress} modifiers={modifiers} />;
      }
      return <Button key={node.testID} label={node.label} systemImage={node.sfSymbol} role={node.destructive ? 'destructive' : undefined} onPress={node.onPress} modifiers={modifiers} />;
    }
  }
}

const MENU_HEIGHT = 46;

function NativeGlassMenuGroup({ menus, scheme }: {
  menus: readonly HeaderMenuSpec[];
  scheme: 'light' | 'dark';
}) {
  const [confirmation, setConfirmation] = useState<MenuConfirmNode | null>(null);

  return (
    <Host matchContents alignment="trailing" colorScheme={scheme} ignoreSafeArea="all">
      <Alert
        title={confirmation?.title ?? ''}
        isPresented={confirmation !== null}
        onIsPresentedChange={(presented) => { if (!presented) setConfirmation(null); }}
      >
        <Alert.Trigger>
          <HStack
            spacing={0}
            modifiers={[
              padding({ horizontal: 7 }),
              glassEffect({
                shape: 'capsule',
                glass: { variant: 'regular', tint: scheme === 'dark' ? DARK_GLASS_CONTROL_TINT : undefined },
              }),
            ]}
          >
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
                  <Text modifiers={[font({ size: 16 }), foregroundStyle('primary'), fixedSize(), padding({ horizontal: 10 }), frame({ minWidth: 76, minHeight: MENU_HEIGHT, maxHeight: MENU_HEIGHT })]}>
                    {menu.label ?? menu.accessibilityLabel}
                  </Text>
                ) : (
                  <Image systemName={menu.sfSymbol} size={17} modifiers={[foregroundStyle('primary'), frame({ width: 40, height: MENU_HEIGHT })]} />
                )}
              >
                {menu.items.map((item) => menuNode(item, setConfirmation))}
              </Menu>
            ))}
          </HStack>
        </Alert.Trigger>
        <Alert.Message>
          <Text>{confirmation?.message}</Text>
        </Alert.Message>
        <Alert.Actions>
          <Button label="Cancel" role="cancel" />
          {confirmation && <Button label={confirmation.confirmLabel} onPress={confirmation.onConfirm} />}
        </Alert.Actions>
      </Alert>
    </Host>
  );
}

export const renderGlassMenuGroup: GlassMenuGroupRenderer = (menus, scheme) => {
  if (!isLiquidGlassAvailable() || !isGlassEffectAPIAvailable()) return undefined;
  return <NativeGlassMenuGroup menus={menus} scheme={scheme} />;
};
