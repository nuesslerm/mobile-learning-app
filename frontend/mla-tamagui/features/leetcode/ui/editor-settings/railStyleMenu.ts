import type { MenuNode } from '@lib/tamagui-design-system';
import { Keyboard } from '@lib/tamagui-design-system/icons';

import { RAIL_STYLES, type RailStyle } from '../../state/editor/symbolBarStore';

export const RAIL_STYLE_LABELS: Record<RailStyle, string> = {
  classic: 'Classic',
  symbols: 'Symbols',
};

export function railStyleMenu(current: RailStyle, select: (style: RailStyle) => void): MenuNode {
  return {
    kind: 'submenu',
    testID: 'ProblemEditorRailStyle',
    icon: Keyboard,
    label: 'Keyboard layout',
    items: RAIL_STYLES.map((style) => ({
      testID: `ProblemEditorRailStyle-${style}`,
      label: RAIL_STYLE_LABELS[style],
      checked: current === style,
      onPress: () => {
        if (style !== current) select(style);
      },
    })),
  };
}
