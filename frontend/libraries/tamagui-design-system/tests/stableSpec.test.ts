import type { LucideIcon } from 'lucide-react-native';
import { describe, expect, it } from 'vitest';

import type { MenuNode } from '../composites/native-header/header-menu-spec';
import type { ScreenHeaderSpec } from '../composites/native-header/header-spec';
import { headerSignature } from '../composites/native-header/screen-header-options';
import {
  collectHandlers,
  withStableHandlers,
} from '../composites/native-header/stable-spec';

const icon = (() => null) as unknown as LucideIcon;

function bar(calls: string[], tag: string): ScreenHeaderSpec {
  const tree: MenuNode[] = [
    {
      testID: 'Wrap',
      label: 'Line wrap',
      checked: true,
      keepOpen: true,
      onPress: () => calls.push(`${tag}:wrap`),
    },
    {
      kind: 'section',
      testID: 'Indent',
      items: [
        {
          kind: 'submenu',
          testID: 'Tabs',
          label: 'Tabs',
          items: [
            {
              testID: 'Tab4',
              label: '4',
              onPress: () => calls.push(`${tag}:tab4`),
            },
          ],
        },
      ],
    },
    {
      kind: 'confirm',
      testID: 'Format',
      label: 'Format Code',
      title: 'Format?',
      message: 'Sure?',
      confirmLabel: 'Format',
      onConfirm: () => calls.push(`${tag}:format`),
    },
  ];
  return {
    testID: 'Bar',
    title: 'Editor',
    onBack: () => calls.push(`${tag}:back`),
    groups: [
      [
        {
          testID: 'Settings',
          accessibilityLabel: 'Settings',
          icon,
          items: tree,
        },
      ],
      [
        {
          testID: 'Info',
          accessibilityLabel: 'Info',
          icon,
          onPress: () => calls.push(`${tag}:info`),
        },
      ],
    ],
  };
}

describe('withStableHandlers and collectHandlers', () => {
  it('Given a spec, When its handlers are collected, Then each is found under a key built from its ids, including those deep in a section and a submenu', () => {
    // Given a spec with a back, an action, and rows nested in a section, a submenu and a confirm
    const calls: string[] = [];

    // When its handlers are collected
    const handlers = collectHandlers(bar(calls, 'a'));
    handlers.get('press:Tab4')?.();
    handlers.get('confirm:Format')?.();
    handlers.get('back')?.();

    // Then the nested ones run too
    expect(calls).toEqual(['a:tab4', 'a:format', 'a:back']);
    expect([...handlers.keys()].sort()).toEqual([
      'back',
      'confirm:Format',
      'press:Info',
      'press:Tab4',
      'press:Wrap',
    ]);
  });

  it('Given a stable copy of a spec, When its handlers are pressed after the spec was replaced, Then the call carries the key and the newest closures run', () => {
    // Given a stable copy whose calls look the handlers up in whatever spec is latest
    const calls: string[] = [];
    let latest = bar(calls, 'old');
    const stable = withStableHandlers(bar(calls, 'old'), (key) =>
      collectHandlers(latest).get(key)?.()
    );

    // When the spec is replaced by one with new closures and the copy's handlers are pressed
    latest = bar(calls, 'new');
    stable.onBack?.();
    const info = stable.groups?.[1]?.[0] as { onPress: () => void };
    info.onPress();

    // Then the newest closures ran, not the ones the copy was made from
    expect(calls).toEqual(['new:back', 'new:info']);
  });

  it('Given a spec and its stable copy, When their signatures are taken, Then they are equal', () => {
    // Given a spec and its copy
    const spec = bar([], 'a');

    // When both are signed
    const stable = withStableHandlers(spec, () => undefined);

    // Then what they display is the same, so a bar built from the copy is not rebuilt
    expect(headerSignature(stable)).toBe(headerSignature(spec));
  });
});
