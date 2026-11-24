import type {
  NativeStackHeaderItem,
  NativeStackHeaderItemMenu,
  NativeStackHeaderItemMenuAction,
  NativeStackHeaderItemMenuSubmenu,
} from 'expo-router';
import type { ReactElement } from 'react';

import type { GlassMenuGroupRenderer } from './glass-menu-group';

import {
  isActionNode,
  type HeaderMenuSpec,
  type MenuNode,
  type SfSymbolName,
} from './header-menu-spec';
import {
  headerGroups,
  isMenuSpec,
  type HeaderActionSpec,
  type HeaderStatusSpec,
  type ScreenHeaderSpec,
} from './header-spec';

export const BUSY_ITEM_SIZE = 32;
export const GROUP_SPACING = 8;

export type NativeHeaderFactories = {
  /** The spinner on its own piece of glass; a bar item is sized by its element, so the size is passed in. */
  busy: (size: number) => ReactElement;
  status?: (status: HeaderStatusSpec) => ReactElement;
  menuGroup?: GlassMenuGroupRenderer;
};

export type NativeHeaderItems = {
  left: NativeStackHeaderItem[];
  /** Left to right: expo-router reverses them itself. */
  right: NativeStackHeaderItem[];
};

type MenuItem =
  | NativeStackHeaderItemMenuAction
  | NativeStackHeaderItemMenuSubmenu;

const BACK_SYMBOL: SfSymbolName = 'chevron.left';

function symbol(name: SfSymbolName | undefined) {
  return name ? ({ type: 'sfSymbol', name } as const) : undefined;
}

function toMenuItem(node: MenuNode): MenuItem {
  if (isActionNode(node)) {
    return {
      type: 'action',
      label: node.label,
      description: node.detail,
      icon: symbol(node.sfSymbol),
      state:
        node.checked === undefined ? undefined : node.checked ? 'on' : 'off',
      disabled: node.disabled,
      destructive: node.destructive,
      keepsMenuPresented: node.keepOpen === true,
      onPress: node.onPress,
    };
  }
  // Every submenu is multiselectable: UIKit's single selection would uncheck the sibling toggle.
  switch (node.kind) {
    case 'section':
      return {
        type: 'submenu',
        label: node.title ?? '',
        inline: true,
        multiselectable: true,
        items: node.items.map(toMenuItem),
      };
    case 'submenu':
      return {
        type: 'submenu',
        label: node.label,
        icon: symbol(node.sfSymbol),
        inline: false,
        destructive: node.destructive,
        multiselectable: true,
        items: node.items.map(toMenuItem),
      };
    case 'confirm':
      return {
        type: 'submenu',
        label: node.label,
        icon: symbol(node.sfSymbol),
        inline: false,
        multiselectable: true,
        items: [
          {
            type: 'action',
            label: node.confirmLabel,
            description: node.message,
            destructive: true,
            keepsMenuPresented: false,
            onPress: node.onConfirm,
          },
        ],
      };
  }
}

function toMenu(
  spec: HeaderMenuSpec,
  sharesBackground: boolean
): NativeStackHeaderItemMenu {
  return {
    type: 'menu',
    label: spec.label ?? spec.accessibilityLabel,
    icon: symbol(spec.sfSymbol),
    accessibilityLabel: spec.accessibilityLabel,
    sharesBackground,
    menu: {
      title: spec.title,
      multiselectable: true,
      items: spec.items.map(toMenuItem),
    },
  };
}

function toButton(
  spec: HeaderActionSpec,
  sharesBackground: boolean
): NativeStackHeaderItem {
  return {
    type: 'button',
    label: spec.accessibilityLabel,
    icon: symbol(spec.sfSymbol),
    accessibilityLabel: spec.accessibilityLabel,
    sharesBackground,
    onPress: spec.onPress,
  };
}

/**
 * The spec as react-native-screens header items. Items of one group share a piece of glass; a group of one gets its own, which a kept-open menu needs
 * (a rebuilt menu on a grouped item collapses). A spacing item between groups keeps the pieces apart.
 */
export function buildNativeHeaderItems(
  spec: ScreenHeaderSpec,
  factories: NativeHeaderFactories,
  scheme: 'light' | 'dark' = 'light'
): NativeHeaderItems {
  const left: NativeStackHeaderItem[] = [];
  const hideBackground = spec.hideNativeControlBackgrounds === true;
  const background = hideBackground ? { hidesSharedBackground: true } : {};
  if (spec.onBack) {
    const label = spec.backAccessibilityLabel ?? 'Go back';
    left.push({
      type: 'button',
      label,
      icon: symbol(BACK_SYMBOL),
      accessibilityLabel: label,
      sharesBackground: false,
      ...background,
      onPress: spec.onBack,
    });
  }

  if (spec.leadingStatus && factories.status) {
    left.push({ type: 'custom', element: factories.status(spec.leadingStatus), hidesSharedBackground: true });
  }
  const right: NativeStackHeaderItem[] = [];
  const separate = () => {
    if (right.length > 0)
      right.push({ type: 'spacing', spacing: GROUP_SPACING });
  };
  if (spec.busy)
    right.push({ type: 'custom', element: factories.busy(BUSY_ITEM_SIZE), ...background });
  for (const group of headerGroups(spec)) {
    separate();
    const shared = group.length > 1;
    const capsule = spec.nativeGlassMenuGroups && shared && group.every(isMenuSpec)
      ? factories.menuGroup?.(group, scheme, hideBackground)
      : undefined;
    if (capsule) {
      right.push({ type: 'custom', element: capsule, hidesSharedBackground: true });
      continue;
    }
    for (const member of group) {
      const item = isMenuSpec(member) ? toMenu(member, shared) : toButton(member, shared);
      right.push({ ...item, ...background });
    }
  }
  return { left, right };
}
