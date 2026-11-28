import type { HomeSettingsGroupProps } from '@lib/skin';
import { InsetGroup } from '@lib/tamagui-design-system';
import { Children, isValidElement } from 'react';

/** The rows of one settings group; the design system's inset group draws the surface and the hairlines. */
export function HomeSettingsGroup(props: HomeSettingsGroupProps) {
  return <InsetGroup testID="skin-settings-group">{Children.toArray(props.children).filter(isValidElement)}</InsetGroup>;
}
