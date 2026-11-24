import { describe, expect, it } from 'vitest';

import type { MenuNode } from '../composites/native-header/header-menu-spec';
import {
  menuNavigation,
  resolveMenuView,
  ROOT_PATH,
} from '../composites/native-header/header-menu-navigation';

const noop = () => undefined;

const tree: MenuNode[] = [
  {
    testID: 'Wrap',
    label: 'Line wrap',
    checked: true,
    keepOpen: true,
    onPress: noop,
  },
  {
    kind: 'section',
    testID: 'Section',
    items: [
      {
        kind: 'submenu',
        testID: 'FontSize',
        label: 'Font size: 16px',
        items: [
          {
            testID: 'Font16',
            label: '16px',
            checked: true,
            keepOpen: true,
            onPress: noop,
          },
          {
            kind: 'submenu',
            testID: 'More',
            label: 'More sizes',
            items: [{ testID: 'Font24', label: '24px', onPress: noop }],
          },
        ],
      },
      {
        kind: 'confirm',
        testID: 'Format',
        label: 'Format Code',
        title: 'Format current code?',
        message: 'Your code will be reformatted.',
        confirmLabel: 'Format',
        onConfirm: noop,
      },
    ],
  },
];

describe('menuNavigation', () => {
  it('Given the root, When a submenu opens, Then the path is that submenu', () => {
    expect(menuNavigation(ROOT_PATH, { type: 'open', id: 'FontSize' })).toEqual(
      ['FontSize']
    );
  });

  it('Given a submenu is open, When its header row is pressed, Then the root shows', () => {
    expect(menuNavigation(['FontSize'], { type: 'back' })).toEqual([]);
  });

  it('Given a submenu inside a submenu, When the header row is pressed, Then the parent submenu shows', () => {
    expect(menuNavigation(['FontSize', 'More'], { type: 'back' })).toEqual([
      'FontSize',
    ]);
  });

  it('Given the root, When the header row is pressed, Then the root stays', () => {
    expect(menuNavigation(ROOT_PATH, { type: 'back' })).toEqual([]);
  });

  it('Given a submenu is open, When an option is picked in it, Then the root shows', () => {
    expect(menuNavigation(['FontSize', 'More'], { type: 'pick' })).toEqual([]);
  });

  it('Given a submenu is open, When the menu closes, Then the root shows the next time', () => {
    expect(menuNavigation(['FontSize'], { type: 'close' })).toEqual([]);
  });
});

describe('resolveMenuView', () => {
  it('Given the root path, When the view is resolved, Then it lists the root items with no header', () => {
    expect(resolveMenuView(tree, ROOT_PATH)).toEqual({
      kind: 'list',
      items: tree,
    });
  });

  it('Given a submenu inside a section, When the view is resolved, Then it lists the submenu with its row as the header', () => {
    const view = resolveMenuView(tree, ['FontSize']);

    expect(view.kind).toBe('list');
    if (view.kind !== 'list') return;
    expect(view.header?.testID).toBe('FontSize');
    expect(view.items.map((node) => node.testID)).toEqual(['Font16', 'More']);
  });

  it('Given a confirm row, When the view is resolved, Then it is the confirmation', () => {
    const view = resolveMenuView(tree, ['Format']);

    expect(view.kind === 'confirm' && view.node.confirmLabel).toBe('Format');
  });

  it('Given a path through a row that is gone, When the view is resolved, Then the root shows', () => {
    expect(resolveMenuView(tree, ['Gone'])).toEqual({
      kind: 'list',
      items: tree,
    });
    expect(resolveMenuView(tree, ['Format', 'Anything'])).toEqual({
      kind: 'list',
      items: tree,
    });
  });

  it('Given a path two submenus deep, When the view is resolved, Then the innermost list shows', () => {
    const view = resolveMenuView(tree, ['FontSize', 'More']);

    expect(
      view.kind === 'list' && view.items.map((node) => node.testID)
    ).toEqual(['Font24']);
  });
});
