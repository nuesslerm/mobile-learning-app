import type { HomeSettingsGroupProps } from '@lib/skin';
import { InsetGroup } from '@lib/tamagui-design-system';
import { Children, isValidElement } from 'react';

/** The rows of one settings group, on one surface, with hairlines between them. */
export function HomeSettingsGroup(props: HomeSettingsGroupProps) {
  return (
    <InsetGroup
      testID="agent-settings-group"
      inset={false}
      separatorInset={props.separatorInset ?? 0}
    >
      {Children.toArray(props.children).filter(isValidElement)}
    </InsetGroup>
  );
}
