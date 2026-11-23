import type { LucideIcon } from 'lucide-react-native';
import { createElement, type ReactElement } from 'react';
import { describe, expect, it, vi } from 'vitest';

import {
  nativeHeaderActive,
  type HeaderEnv,
} from '../composites/native-header/header-env';
import type {
  HeaderMenuSpec,
  MenuNode,
} from '../composites/native-header/header-menu-spec';
import type { ScreenHeaderSpec } from '../composites/native-header/header-spec';
import {
  buildScreenHeaderOptions,
  countHeaderControls,
  headerSignature,
  titleMaxWidth,
  type HeaderRenderers,
  type HeaderTitleProps,
} from '../composites/native-header/screen-header-options';

const icon = (() => null) as unknown as LucideIcon;
const noop = () => undefined;

const NATIVE: HeaderEnv = {
  platform: 'ios',
  nativeHeaders: true,
  optedIn: true,
};

function renderers(): HeaderRenderers {
  return {
    classic: vi.fn((spec: ScreenHeaderSpec) =>
      createElement('classic', { id: spec.testID })
    ),
    title: vi.fn((props: HeaderTitleProps) =>
      createElement('glass-title', props)
    ),
    busy: vi.fn((size: number) => createElement('busy', { size })),
  };
}

function settings(
  checked: boolean,
  onPress: () => void = noop
): HeaderMenuSpec {
  const items: MenuNode[] = [
    { testID: 'Wrap', label: 'Line wrap', checked, keepOpen: true, onPress },
  ];
  return {
    testID: 'Settings',
    accessibilityLabel: 'Editor settings',
    label: 'Python3',
    sfSymbol: 'slider.horizontal.3',
    icon,
    items,
  };
}

function editorSpec(
  checked = true,
  onPress: () => void = noop
): ScreenHeaderSpec {
  return {
    title: 'Two Sum',
    subtitle: 'Arrays · Easy',
    onBack: noop,
    busy: true,
    groups: [
      [settings(checked, onPress)],
      [
        {
          icon,
          sfSymbol: 'info.circle',
          accessibilityLabel: 'Description',
          onPress: noop,
          testID: 'Info',
        },
      ],
    ],
    testID: 'Header',
  };
}

function options(env: HeaderEnv, spec = editorSpec(), r = renderers()) {
  return buildScreenHeaderOptions({
    spec,
    env,
    scheme: 'dark',
    theme: { background: '#101010', tint: '#fafafa' },
    windowWidth: 390,
    renderers: r,
  });
}

describe('nativeHeaderActive', () => {
  const platforms = ['ios', 'android', 'web'];
  const flags = [true, false];

  const table = platforms.flatMap((platform) =>
    flags.flatMap((nativeHeaders) =>
      flags.map((optedIn) => ({
        platform,
        nativeHeaders,
        optedIn,
        active: platform === 'ios' && nativeHeaders && optedIn,
      }))
    )
  );

  it.each(table)(
    'Given platform $platform, setting $nativeHeaders and opted in $optedIn, When asked, Then the native bar is active: $active',
    ({ active, ...env }) => {
      expect(nativeHeaderActive(env)).toBe(active);
    }
  );

  it('Given the whole table, When it is read, Then exactly one combination is active', () => {
    expect(table.filter((row) => row.active)).toHaveLength(1);
  });
});

describe('buildScreenHeaderOptions', () => {
  it('Given ios, the setting on and the screen opted in, When options are built, Then the bar is native, opaque and themed, with a title and both item functions', () => {
    const r = renderers();

    const built = options(NATIVE, editorSpec(), r);

    expect(built).toMatchObject({
      headerShown: true,
      headerTransparent: false,
      headerBackVisible: false,
      headerShadowVisible: false,
      headerStyle: { backgroundColor: '#101010' },
      headerTintColor: '#fafafa',
      headerUserInterfaceStyle: 'dark',
      title: 'Two Sum',
    });
    expect(built.header).toBeUndefined();
    expect(typeof built.headerTitle).toBe('function');
    expect(built.unstable_headerLeftItems?.({})).toHaveLength(1);
    expect(
      built.unstable_headerRightItems?.({}).map((item) => item.type)
    ).toEqual(['custom', 'spacing', 'menu', 'spacing', 'button']);
    expect(r.classic).not.toHaveBeenCalled();
  });

  it('Given the native bar, When the title is drawn, Then the injected factory gets the title, the subtitle and a width the controls leave', () => {
    const r = renderers();
    const built = options(NATIVE, editorSpec(), r);

    const title = (built.headerTitle as () => ReactElement)();

    expect(title.type).toBe('glass-title');
    expect(title.props).toEqual({
      title: 'Two Sum',
      subtitle: 'Arrays · Easy',
      maxWidth: titleMaxWidth(390, countHeaderControls(editorSpec())),
    });
  });

  it('Given a light scheme, When options are built, Then the bar is told it is light', () => {
    const built = buildScreenHeaderOptions({
      spec: editorSpec(),
      env: NATIVE,
      scheme: 'light',
      theme: { background: '#ffffff', tint: '#000000' },
      windowWidth: 390,
      renderers: renderers(),
    });

    expect(built.headerUserInterfaceStyle).toBe('light');
  });

  it.each<[string, HeaderEnv]>([
    ['android', { ...NATIVE, platform: 'android' }],
    ['web', { ...NATIVE, platform: 'web' }],
    ['the setting off', { ...NATIVE, nativeHeaders: false }],
    ['a screen that did not opt in', { ...NATIVE, optedIn: false }],
  ])(
    'Given %s, When options are built, Then only the header function is set, with none of the unstable_ keys',
    (_name, env) => {
      const r = renderers();
      const spec = editorSpec();

      const built = options(env, spec, r);

      expect(Object.keys(built)).toEqual(['header']);
      const rendered = (built.header as () => ReactElement)();
      expect(rendered.type).toBe('classic');
      expect(r.classic).toHaveBeenCalledWith(spec);
    }
  );
});

describe('headerSignature', () => {
  it('Given the same data under new closure identities, When the signatures are taken, Then they are equal', () => {
    expect(headerSignature(editorSpec(true, () => undefined))).toBe(
      headerSignature(editorSpec(true, () => undefined))
    );
  });

  it('Given a checked row flips, When the signatures are taken, Then they differ', () => {
    expect(headerSignature(editorSpec(true))).not.toBe(
      headerSignature(editorSpec(false))
    );
  });

  it('Given the title, the subtitle or the busy status changes, When the signatures are taken, Then each differs', () => {
    const base = headerSignature(editorSpec());

    expect(
      headerSignature({ ...editorSpec(), title: 'Add Two Numbers' })
    ).not.toBe(base);
    expect(headerSignature({ ...editorSpec(), subtitle: undefined })).not.toBe(
      base
    );
    expect(headerSignature({ ...editorSpec(), busy: false })).not.toBe(base);
    expect(headerSignature({ ...editorSpec(), onBack: undefined })).not.toBe(
      base
    );
  });
});

describe('titleMaxWidth', () => {
  it('Given no controls, When the width is read, Then the title may take the whole window', () => {
    expect(titleMaxWidth(390, { left: 0, right: 0 })).toBe(390);
  });

  it('Given a back button and one control, When the width is read, Then both flanks are the same and the title is centred', () => {
    expect(titleMaxWidth(390, { left: 1, right: 1 })).toBe(270);
  });

  it('Given more controls on one side, When the width is read, Then the busier side sets both flanks', () => {
    expect(titleMaxWidth(390, { left: 1, right: 2 })).toBe(166);
    expect(titleMaxWidth(390, { left: 2, right: 1 })).toBe(166);
  });

  it('Given more controls than the window holds, When the width is read, Then it never goes below the floor', () => {
    expect(titleMaxWidth(390, { left: 1, right: 4 })).toBe(96);
    expect(titleMaxWidth(100, { left: 0, right: 0 })).toBe(100);
    expect(titleMaxWidth(60, { left: 0, right: 0 })).toBe(96);
  });

  it('Given a spec, When its controls are counted, Then the back, the busy spinner and every group member count', () => {
    expect(countHeaderControls(editorSpec())).toEqual({ left: 1, right: 3 });
    expect(countHeaderControls({ title: 'Plain', testID: 'Header' })).toEqual({
      left: 0,
      right: 0,
    });
  });
});
