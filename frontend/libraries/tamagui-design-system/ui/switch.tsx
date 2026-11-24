import { Logger } from '@lib/logging';
import { Platform, Switch as RNSwitch } from 'react-native';

import { useTheme } from '../primitives';

export type SwitchProps = {
  value: boolean;
  onValueChange: (value: boolean) => void;
  /** Read aloud by a screen reader, since the switch has no text of its own. */
  accessibilityLabel: string;
  testID: string;
  disabled?: boolean;
};

/** The platform's own switch (the green track of iOS's settings), with the theme's colours where the platform lets them be set. */
export function Switch({ value, onValueChange, accessibilityLabel, testID, disabled }: SwitchProps) {
  const theme = useTheme();
  return (
    <RNSwitch
      testID={testID}
      accessibilityLabel={accessibilityLabel}
      value={value}
      disabled={disabled}
      trackColor={{ false: theme.color5?.val ?? '#bbb', true: theme.green9?.val ?? '#34c759' }}
      thumbColor="#fff"
      // react-native-web paints the thumb of an on switch teal unless this web-only prop says otherwise; the phone has one thumb colour
      {...(Platform.OS === 'web' ? { activeThumbColor: '#fff' } : {})}
      onValueChange={(next) => {
        Logger.info('ds.switch.change', { testID, value: next });
        onValueChange(next);
      }}
    />
  );
}
