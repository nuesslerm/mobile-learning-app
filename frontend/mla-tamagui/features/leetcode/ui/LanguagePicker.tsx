import { Logger } from '@lib/logging';
import { Button, Text } from '@lib/tamagui-design-system';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import WheelPicker from '@quidone/react-native-wheel-picker';
import { StyleSheet, View } from 'react-native';
import { useTheme } from '@lib/tamagui-design-system/primitives';

import { wheelItemContainer } from './WheelItemContainer';

export type EditorLanguage = 'python3' | 'javascript';

const LANGUAGE_LABELS: Record<EditorLanguage, string> = {
  python3: 'Python',
  javascript: 'JavaScript',
};

const LANGUAGES: EditorLanguage[] = ['python3', 'javascript'];

const ITEMS = LANGUAGES.map((value) => ({
  value,
  label: LANGUAGE_LABELS[value],
}));

export type LanguagePickerProps = {
  value: EditorLanguage;
  onOpen: () => void;
  disabled?: boolean;
  testID?: string;
};

/** The in-row control: shows the current language and asks for the dial. */
export function LanguagePicker({
  value,
  onOpen,
  disabled = false,
  testID = 'LanguagePicker',
}: LanguagePickerProps) {
  return (
    <Button
      testID={`${testID}-Trigger`}
      variant="outline"
      size="$2"
      disabled={disabled}
      pressLog="off"
      onPress={() => {
        Logger.info('editor.language_picker.open', { lang: value });
        onOpen();
      }}
    >
      {LANGUAGE_LABELS[value]}
    </Button>
  );
}

export type LanguageSheetProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  value: EditorLanguage;
  onChange: (lang: EditorLanguage) => void;
  testID?: string;
};

/**
 * A dial rather than a row of buttons: two languages fit in a toggle, but the
 * backend's language list is the thing that grows, and a wheel is the one
 * control that does not need re-laying-out when it does.
 *
 * Rendered by the SCREEN, at the end of its own tree, not beside the trigger.
 * The sheet does not portal (deliberately -- a portalled sheet under a
 * transparentModal route swallows every touch), so it only covers what comes
 * before it in the tree. Mounted next to the trigger it sat mid-tree and the
 * editor's test-input row painted straight over the wheel.
 *
 * `enableScrollByTapOnItem` is what keeps this drivable without a gesture: a
 * tap on a row scrolls the wheel to it, so the same per-language testIDs the
 * old button row published still select a language, in a browser check as much
 * as under a thumb.
 */
export function LanguageSheet({
  open,
  onOpenChange,
  value,
  onChange,
  testID = 'LanguagePicker',
}: LanguageSheetProps) {
  // The wheel ships no colours of its own, so its rows fall back to React
  // Native's default black text -- invisible on the dark theme, which is
  // exactly how this shipped: a sheet titled "Language" over what looked like
  // an empty panel, with Python and JavaScript there but unreadable. Rows are
  // rendered with the DS Text rather than RN's, so they take `$color` the same
  // way every other label in the app does instead of a literal that is only
  // right on one theme.
  const theme = useTheme();
  const selectionColor = theme.color4?.val ?? 'rgba(128,128,128,0.22)';

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      // Sized to the dial, not to a round number. At 45% the wheel sat in a
      // half-empty panel with a void between it and the header, which is what
      // made the sheet read as "hanging in the middle of the screen".
      snapPoints={[32]}
      // The wheel brings its own ScrollView, which is not one of gorhom's --
      // leave the sheet's own pan on and the wheel cannot be spun.
      disableContentPanningGesture
      testID={`${testID}Sheet`}
    >
      <SheetHeader onClose={() => onOpenChange(false)} testID={`${testID}Header`}>
        <Text variant="h4">Language</Text>
      </SheetHeader>
      <View style={styles.wheel}>
        <WheelPicker
          data={ITEMS}
          value={value}
          width="100%"
          // Three rows, not the default five: with two languages the extra rows
          // are blank space, and the dial reads as broken rather than short.
          visibleItemCount={3}
          enableScrollByTapOnItem
          // The library dims the neighbour row to opacity 0.2, which with two
          // languages is the *other* language. See WheelItemContainer.
          renderItemContainer={wheelItemContainer}
          testID={`${testID}Wheel`}
          // The selection band. Without it nothing marks which row is chosen,
          // so the wheel reads as a plain list that happens to be centred.
          overlayItemStyle={{
            backgroundColor: selectionColor,
            borderRadius: 10,
            marginHorizontal: 16,
          }}
          renderItem={({ item, itemTextStyle }) => (
            <Text
              testID={`${testID}-${item.label}`}
              style={[styles.item, itemTextStyle]}
            >
              {item.label}
            </Text>
          )}
          onValueChanged={({ item }) => {
            if (item.value === value) return;
            Logger.info('editor.language_picker.tap', { lang: item.value });
            onChange(item.value);
            onOpenChange(false);
          }}
        />
      </View>
    </Sheet>
  );
}

const styles = StyleSheet.create({
  // Directly under the header rather than centred in whatever height is left:
  // a dial that drifts away from its own title is the part that looked wrong.
  wheel: { paddingTop: 8 },
  item: { textAlign: 'center', fontSize: 20, lineHeight: 48 },
});
