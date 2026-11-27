// Ported from `references/orca/mobile/src/components/list/ListGroup.tsx` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
import type { HomeSettingsGroupProps } from '@lib/skin';
import { Children, Fragment, isValidElement } from 'react';
import { View } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { useHomeTabStyles } from './homeTabStyles';

/** The rows of one settings group, on one surface, with hairlines between them. */
export function HomeSettingsGroup(props: HomeSettingsGroupProps) {
  const styles = useHomeTabStyles();
  const colors = homePalettes[useOrcaScheme()];
  const rows = Children.toArray(props.children).filter(isValidElement);
  return (
    <View style={[styles.settingsGroup, { backgroundColor: colors.bgGroup }]}>
      {rows.map((row, index) => (
        <Fragment key={index}>
          {index > 0 ? (
            <View
              style={[styles.settingsSeparator, { marginLeft: props.separatorInset ?? 0 }]}
            />
          ) : null}
          {row}
        </Fragment>
      ))}
    </View>
  );
}
