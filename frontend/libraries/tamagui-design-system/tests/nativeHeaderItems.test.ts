import type {
  NativeStackHeaderItem,
  NativeStackHeaderItemMenu,
  NativeStackHeaderItemMenuSubmenu,
} from 'expo-router';
import type { LucideIcon } from 'lucide-react-native';
import { createElement } from 'react';
import { describe, expect, it, vi } from 'vitest';

import type {
  HeaderMenuSpec,
  MenuNode,
} from '../composites/native-header/header-menu-spec';
import type { ScreenHeaderSpec } from '../composites/native-header/header-spec';
import {
  BUSY_ITEM_SIZE,
  GROUP_SPACING,
  buildNativeHeaderItems,
} from '../composites/native-header/native-header-items';

const icon = (() => null) as unknown as LucideIcon;
const busyFactory = vi.fn((size: number) => createElement('busy', { size }));

function build(spec: ScreenHeaderSpec) {
  return buildNativeHeaderItems(spec, { busy: busyFactory });
}

function menuItem(
  item: NativeStackHeaderItem | undefined
): NativeStackHeaderItemMenu {
  if (item?.type !== 'menu')
    throw new Error(`expected a menu item, got ${item?.type}`);
  return item;
}

function submenu(item: unknown): NativeStackHeaderItemMenuSubmenu {
  const candidate = item as NativeStackHeaderItemMenuSubmenu | undefined;
  if (candidate?.type !== 'submenu')
    throw new Error(`expected a submenu, got ${candidate?.type}`);
  return candidate;
}

function menuSpec(
  items: MenuNode[],
  overrides: Partial<HeaderMenuSpec> = {}
): HeaderMenuSpec {
  return {
    testID: 'Menu',
    accessibilityLabel: 'Editor settings',
    icon,
    items,
    ...overrides,
  };
}

const noop = () => undefined;

describe('buildNativeHeaderItems', () => {
  it('Given back, groups of settings and of info and more, and busy, When items are built, Then the bar reads left to right as one back button and the glass pieces apart', () => {
    const onBack = vi.fn();
    const info = vi.fn();
    const settings = menuSpec([], {
      testID: 'Settings',
      accessibilityLabel: 'Editor settings',
      label: 'Python3',
      sfSymbol: 'slider.horizontal.3',
    });
    const more = menuSpec([], {
      testID: 'More',
      accessibilityLabel: 'More',
      sfSymbol: 'ellipsis',
    });

    const { left, right } = build({
      title: 'Two Sum',
      onBack,
      busy: true,
      groups: [
        [settings],
        [
          {
            icon,
            sfSymbol: 'info.circle',
            accessibilityLabel: 'Description',
            onPress: info,
            testID: 'Info',
          },
          more,
        ],
      ],
      testID: 'Header',
    });

    expect(left).toHaveLength(1);
    expect(left[0]).toMatchObject({
      type: 'button',
      icon: { type: 'sfSymbol', name: 'chevron.left' },
      accessibilityLabel: 'Go back',
    });
    (left[0] as { onPress: () => void }).onPress();
    expect(onBack).toHaveBeenCalledTimes(1);

    expect(right.map((item) => item.type)).toEqual([
      'custom',
      'spacing',
      'menu',
      'spacing',
      'button',
      'menu',
    ]);
    expect(busyFactory).toHaveBeenCalledWith(BUSY_ITEM_SIZE);
    expect(BUSY_ITEM_SIZE).toBe(32);
    expect(right[1]).toEqual({ type: 'spacing', spacing: GROUP_SPACING });
    expect(menuItem(right[2]).sharesBackground).toBe(false);
    expect(right[4]).toMatchObject({
      type: 'button',
      sharesBackground: true,
      accessibilityLabel: 'Description',
    });
    (right[4] as { onPress: () => void }).onPress();
    expect(info).toHaveBeenCalledTimes(1);
    expect(menuItem(right[5]).sharesBackground).toBe(true);
  });

  it('Given no busy status, When items are built, Then there is no spinner and no leading spacing', () => {
    const { right } = build({
      title: 'Plain',
      groups: [[menuSpec([])]],
      testID: 'Header',
    });

    expect(right.map((item) => item.type)).toEqual(['menu']);
  });

  it('Given no onBack, When items are built, Then there is no back item', () => {
    expect(build({ title: 'Plain', testID: 'Header' }).left).toEqual([]);
  });

  it('Given a back label, When items are built, Then the back item carries it for accessibility', () => {
    const { left } = build({
      title: 'Plain',
      onBack: noop,
      backAccessibilityLabel: 'Back to Settings',
      testID: 'Header',
    });

    expect(left[0]).toMatchObject({
      accessibilityLabel: 'Back to Settings',
      label: 'Back to Settings',
    });
  });

  it("Given today's actions and menu with no groups, When items are built, Then they share one piece of glass with no spacing", () => {
    const { right } = build({
      title: 'Two Sum',
      actions: [
        { icon, accessibilityLabel: 'Info', onPress: noop, testID: 'Info' },
      ],
      menu: menuSpec([]),
      testID: 'Header',
    });

    expect(right.map((item) => item.type)).toEqual(['button', 'menu']);
    expect(
      right.map(
        (item) => (item as { sharesBackground?: boolean }).sharesBackground
      )
    ).toEqual([true, true]);
  });

  it('Given a menu with a label and an icon, When items are built, Then both are passed along with the accessibility label', () => {
    const { right } = build({
      title: 'Two Sum',
      groups: [
        [
          menuSpec([], {
            label: 'Python3',
            sfSymbol: 'slider.horizontal.3',
            accessibilityLabel: 'Editor settings',
          }),
        ],
      ],
      testID: 'Header',
    });

    expect(menuItem(right[0])).toMatchObject({
      label: 'Python3',
      icon: { type: 'sfSymbol', name: 'slider.horizontal.3' },
      accessibilityLabel: 'Editor settings',
    });
  });

  it('Given a menu with no label, When items are built, Then the accessibility label is its label', () => {
    const { right } = build({
      title: 'Two Sum',
      groups: [[menuSpec([], { accessibilityLabel: 'More' })]],
      testID: 'Header',
    });

    expect(menuItem(right[0]).label).toBe('More');
  });
});

describe('the menu tree on a native menu', () => {
  function rootOf(items: MenuNode[]) {
    return menuItem(
      build({ title: 'Two Sum', groups: [[menuSpec(items)]], testID: 'Header' })
        .right[0]
    );
  }

  it('Given a checked toggle and an unchecked one, When items are built, Then the actions are on and off and keep the menu presented', () => {
    const onWrap = vi.fn();
    const root = rootOf([
      {
        testID: 'Wrap',
        label: 'Line wrap',
        sfSymbol: 'text.append',
        checked: true,
        keepOpen: true,
        onPress: onWrap,
      },
      {
        testID: 'Space',
        label: 'Show Whitespace',
        checked: false,
        keepOpen: true,
        onPress: noop,
      },
    ]);

    expect(root.menu.items[0]).toMatchObject({
      type: 'action',
      label: 'Line wrap',
      state: 'on',
      keepsMenuPresented: true,
      icon: { type: 'sfSymbol', name: 'text.append' },
    });
    expect(root.menu.items[1]).toMatchObject({
      type: 'action',
      state: 'off',
      keepsMenuPresented: true,
    });
    (root.menu.items[0] as { onPress: () => void }).onPress();
    expect(onWrap).toHaveBeenCalledTimes(1);
  });

  it('Given option nodes in a submenu, When items are built, Then the checked one is on, the picks keep the menu presented, the submenu is not inline and every menu is multiselectable', () => {
    const root = rootOf([
      {
        kind: 'submenu',
        testID: 'FontSize',
        label: 'Font size: 16px',
        items: [
          {
            testID: 'Font14',
            label: '14px',
            checked: false,
            keepOpen: true,
            onPress: noop,
          },
          {
            testID: 'Font16',
            label: '16px',
            checked: true,
            keepOpen: true,
            onPress: noop,
          },
        ],
      },
    ]);

    const fontSize = submenu(root.menu.items[0]);
    expect(root.menu.multiselectable).toBe(true);
    expect(fontSize).toMatchObject({
      label: 'Font size: 16px',
      inline: false,
      multiselectable: true,
    });
    expect(
      fontSize.items.map((item) => (item as { state?: string }).state)
    ).toEqual(['off', 'on']);
    expect(
      fontSize.items.map(
        (item) => (item as { keepsMenuPresented?: boolean }).keepsMenuPresented
      )
    ).toEqual([true, true]);
  });

  it('Given a section, When items are built, Then it is an inline submenu with its rows', () => {
    const root = rootOf([
      {
        kind: 'section',
        testID: 'Rows',
        title: 'Layout',
        items: [{ testID: 'Wrap', label: 'Line wrap', onPress: noop }],
      },
    ]);

    const section = submenu(root.menu.items[0]);
    expect(section).toMatchObject({
      label: 'Layout',
      inline: true,
      multiselectable: true,
    });
    expect(section.items).toHaveLength(1);
  });

  it('Given a confirm node, When items are built, Then it is a submenu whose one destructive action carries the message, runs onConfirm and lets the menu close', () => {
    const onConfirm = vi.fn();
    const root = rootOf([
      {
        kind: 'confirm',
        testID: 'Format',
        label: 'Format Code',
        sfSymbol: 'wand.and.sparkles',
        title: 'Format current code?',
        message: 'Your code will be reformatted.',
        confirmLabel: 'Format',
        onConfirm,
      },
    ]);

    const format = submenu(root.menu.items[0]);
    expect(format).toMatchObject({
      label: 'Format Code',
      inline: false,
      icon: { type: 'sfSymbol', name: 'wand.and.sparkles' },
    });
    expect(format.items).toHaveLength(1);
    expect(format.items[0]).toMatchObject({
      type: 'action',
      label: 'Format',
      description: 'Your code will be reformatted.',
      destructive: true,
      keepsMenuPresented: false,
    });
    (format.items[0] as { onPress: () => void }).onPress();
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it('Given a plain action with a detail, When items are built, Then it does not keep the menu presented, has no state and shows the detail as its description', () => {
    const root = rootOf([
      {
        testID: 'History',
        label: 'Version history',
        detail: '3 saved',
        disabled: true,
        onPress: noop,
      },
    ]);

    expect(root.menu.items[0]).toMatchObject({
      type: 'action',
      label: 'Version history',
      description: '3 saved',
      disabled: true,
      keepsMenuPresented: false,
    });
    expect((root.menu.items[0] as { state?: string }).state).toBeUndefined();
  });

  it("Given today's HeaderMenuItem rows with no kind, When items are built, Then they become plain actions", () => {
    const root = rootOf([
      { icon, label: 'Login', onPress: noop, testID: 'Login' },
    ]);

    expect(root.menu.items[0]).toMatchObject({
      type: 'action',
      label: 'Login',
      keepsMenuPresented: false,
    });
  });
});
