// Ported from `references/orca/mobile/src/components/MobileSearchField.tsx` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
import type { HomeSearchFieldProps } from '@lib/agent-ui-contracts';
import { Pressable, TextInput, View } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { ClearIcon, SearchIcon } from './homeIcons';
import { useHomeTabStyles } from './homeTabStyles';

/** The native search field of the agents tab. */
export function HomeSearchField(props: HomeSearchFieldProps) {
  const styles = useHomeTabStyles();
  const colors = homePalettes[useOrcaScheme()];
  return (
    <View style={styles.search}>
      <SearchIcon size={16} color={colors.textMuted} />
      <TextInput
        style={styles.searchInput}
        value={props.value}
        onChangeText={props.onChangeText}
        placeholder={props.placeholder}
        placeholderTextColor={colors.textMuted}
        autoCapitalize="none"
        autoCorrect={false}
        autoFocus
        accessibilityLabel={props.placeholder}
        testID="HomeSearchField"
      />
      {props.value.length > 0 ? (
        <Pressable
          style={styles.searchClear}
          accessibilityRole="button"
          accessibilityLabel="Clear search"
          onPress={props.onClear}
        >
          <ClearIcon size={16} color={colors.textMuted} />
        </Pressable>
      ) : null}
    </View>
  );
}
