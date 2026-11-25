import type { LucideIcon } from 'lucide-react-native';
import { describe, expect, it } from 'vitest';

import {
  headerGroups,
  isMenuSpec,
  type ScreenHeaderSpec,
} from '../composites/native-header/header-spec';
import {
  isActionNode,
  type MenuActionNode,
  menuSignature,
  validateMenuTree,
  type HeaderMenuSpec,
  type MenuNode,
} from '../composites/native-header/header-menu-spec';
import type { ScreenHeaderProps } from '../composites/screen-header';

const icon = (() => null) as unknown as LucideIcon;

function settingsTree(
  onPress: () => void,
  wrapChecked = true,
  detail = 'Soft wrap'
): MenuNode[] {
  return [
    {
      testID: 'Wrap',
      label: 'Line wrap',
      detail,
      sfSymbol: 'text.append',
      checked: wrapChecked,
      keepOpen: true,
      onPress,
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
              onPress,
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
          onConfirm: onPress,
        },
      ],
    },
  ];
}

describe('menuSignature', () => {
  it('Given two trees equal in data and different in closure identities, When their signatures are taken, Then they are equal', () => {
    const first = settingsTree(() => undefined);
    const second = settingsTree(() => undefined);

    expect(menuSignature(first)).toBe(menuSignature(second));
  });

  it('Given a toggle flips checked, When the signatures are taken, Then they differ', () => {
    expect(menuSignature(settingsTree(() => undefined, true))).not.toBe(
      menuSignature(settingsTree(() => undefined, false))
    );
  });

  it('Given a detail text changes, When the signatures are taken, Then they differ', () => {
    expect(
      menuSignature(settingsTree(() => undefined, true, 'Soft wrap'))
    ).not.toBe(menuSignature(settingsTree(() => undefined, true, 'Hard wrap')));
  });

  it('Given an action image changes, When the signatures are taken, Then the native menu refreshes', () => {
    const action: MenuActionNode = {
      testID: 'wrap',
      label: 'Line wrap',
      checked: true,
      onPress: () => undefined,
    };
    const withImage: MenuActionNode = {
      ...action,
      iosMenuImage: {
        light: 'data:image/png;base64,light',
        dark: 'data:image/png;base64,dark',
      },
    };

    expect(menuSignature([withImage])).not.toBe(menuSignature([action]));
  });

  it('Given a checked row deep inside a submenu flips, When the signatures are taken, Then they differ', () => {
    const base = settingsTree(() => undefined);
    const flipped = settingsTree(() => undefined);
    const section = flipped[1];
    if (section?.kind !== 'section' || section.items[0]?.kind !== 'submenu')
      throw new Error('fixture shape');
    const option = section.items[0].items[0];
    if (!option || !isActionNode(option)) throw new Error('fixture shape');
    option.checked = false;

    expect(menuSignature(base)).not.toBe(menuSignature(flipped));
  });
});

describe('validateMenuTree', () => {
  it('Given ids that are all different, When validated, Then the tree is valid', () => {
    expect(validateMenuTree(settingsTree(() => undefined))).toEqual({
      valid: true,
      duplicateIds: [],
    });
  });

  it('Given duplicate node ids, even in different submenus, When validated, Then the duplicate is named once', () => {
    const tree: MenuNode[] = [
      { testID: 'Dup', label: 'One', onPress: () => undefined },
      {
        kind: 'submenu',
        testID: 'Sub',
        label: 'Sub',
        items: [
          { testID: 'Dup', label: 'Two', onPress: () => undefined },
          { testID: 'Dup', label: 'Three', onPress: () => undefined },
        ],
      },
    ];

    expect(validateMenuTree(tree)).toEqual({
      valid: false,
      duplicateIds: ['Dup'],
    });
  });
});

describe('the header spec', () => {
  it("Given today's ScreenHeaderProps, When it is used as a spec, Then it is accepted and its actions and menu form one group", () => {
    const item: MenuActionNode = {
      icon,
      label: 'Format',
      onPress: () => undefined,
      testID: 'Menu-Format',
    };
    const props: ScreenHeaderProps = {
      title: 'Two Sum',
      subtitle: 'Arrays · Easy',
      onBack: () => undefined,
      actions: [
        {
          icon,
          accessibilityLabel: 'Info',
          onPress: () => undefined,
          testID: 'Info',
        },
      ],
      menu: { icon, accessibilityLabel: 'More', items: [item], testID: 'More' },
      testID: 'Header',
    };

    const spec: ScreenHeaderSpec = props;

    const groups = headerGroups(spec);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.map((member) => member.testID)).toEqual(['Info', 'More']);
    expect(groups[0]?.map(isMenuSpec)).toEqual([false, true]);
  });

  it('Given groups, When the groups are read, Then they replace the actions and an empty group is dropped', () => {
    const settings: HeaderMenuSpec = {
      icon,
      accessibilityLabel: 'Settings',
      items: [],
      testID: 'Settings',
    };
    const spec: ScreenHeaderSpec = {
      title: 'Two Sum',
      actions: [
        {
          icon,
          accessibilityLabel: 'Ignored',
          onPress: () => undefined,
          testID: 'Ignored',
        },
      ],
      groups: [[settings], []],
      testID: 'Header',
    };

    expect(
      headerGroups(spec).map((group) => group.map((member) => member.testID))
    ).toEqual([['Settings']]);
  });

  it('Given a spec with neither actions nor menu, When the groups are read, Then there are none', () => {
    expect(headerGroups({ title: 'Plain', testID: 'Header' })).toEqual([]);
  });
});
