import { Button, SegmentedControl, Text } from '@lib/tamagui-design-system';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { Trash2 } from '@lib/tamagui-design-system/icons';
import { YStack } from '@lib/tamagui-design-system/primitives';
import React from 'react';
import { Keyboard, Platform, Pressable, StyleSheet, View, useWindowDimensions } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { FullWindowOverlay } from 'react-native-screens';

import { captionsOf } from '../../domain/console/caseFields';
import { CONSOLE_SNAP_PERCENT, FLOATING_CONSOLE_SNAP_PERCENT } from '../../domain/console/layout';
import type { Blind75Problem } from '../../domain/models';
import type { ConsoleState, ConsoleTab } from '../../state/console/useConsole';
import { CaseChips } from './CaseChips';
import { CaseFields } from './CaseFields';

const SNAP_POINTS = [CONSOLE_SNAP_PERCENT];
const FLOATING_SNAP_POINTS = [FLOATING_CONSOLE_SNAP_PERCENT];
const FLOATING_CONTENT = { gap: 4, paddingHorizontal: 16, paddingTop: 0, paddingBottom: 38 };
const FLOATING_EDIT_CONTENT = { ...FLOATING_CONTENT, gap: 0 };
const CONSOLE_HANDLE = { width: 60, height: 4, backgroundColor: 'rgba(175,175,185,0.47)', transform: [{ translateY: -4 }] } as const;
const CASE_CARD = '#1d1c1f';
const CONSOLE_TAB_TRACK = { backgroundColor: '#222227', transform: [{ translateY: -4 }] };
const CONSOLE_TAB_THUMB = { backgroundColor: '#595860', borderRadius: 16 };
const BACKDROP_OPACITY = 0.5;
const TABS = [
  { value: 'testcase', label: 'Testcase' },
  { value: 'result', label: 'Run Result' },
];

type Props = {
  problem: Blind75Problem;
  console: ConsoleState;
  colorScheme: 'light' | 'dark';
  fieldError: { name: string; message: string } | null;
  result: React.ReactNode;
  footer: React.ReactNode;
  onSheetClosed: () => void;
  editorHeightBelowNativeHeader?: number;
  nativeHeaderBottom?: number;
};

const floating = Platform.OS === 'ios';
export function ConsoleSheet({ problem, console: state, colorScheme, fieldError, result, footer, onSheetClosed, editorHeightBelowNativeHeader, nativeHeaderBottom }: Props) {
  const captions = React.useMemo(() => captionsOf(problem), [problem]);
  const [focusToken, setFocusToken] = React.useState(0);
  const [editing, setEditing] = React.useState(false);
  const { selected } = state;
  const { height: windowHeight } = useWindowDimensions();
  const { top } = useSafeAreaInsets();
  const headerBottom = nativeHeaderBottom ?? (editorHeightBelowNativeHeader ? windowHeight - editorHeightBelowNativeHeader : 0);
  const headerHeight = Math.max(0, headerBottom - top);
  const darkGlass = floating && colorScheme === 'dark';
  React.useEffect(() => {
    if (!state.open) setEditing(false);
  }, [state.open]);

  const add = () => {
    if (!state.addCase()) return;
    if (!floating) setFocusToken((token) => token + 1);
  };
  const edit = () => {
    if (floating) {
      if (editing) Keyboard.dismiss();
      setEditing(!editing);
    } else if (selected?.kind === 'bundled') add();
    else setFocusToken((token) => token + 1);
  };

  const sheet = (
    <Sheet
      open={state.open}
      onOpenChange={(open) => !open && onSheetClosed()}
      snapPoints={floating ? FLOATING_SNAP_POINTS : SNAP_POINTS}
      backdrop={floating ? 'dim' : 'none'}
      backdropOpacity={BACKDROP_OPACITY}
      floating={floating}
      floatingHorizontalInset={8}
      floatingBottomInset={8}
      cornerRadius={floating ? 38 : undefined}
      bottomCornerRadius={floating ? 46 : undefined}
      glassColorScheme={floating ? colorScheme : undefined}
      glassTintColor={darkGlass ? 'rgba(10, 10, 10, 0.17)' : undefined}
      glassOverlayColor={darkGlass ? 'rgba(0,0,0,0.40)' : undefined}
      handleIndicatorStyle={floating ? CONSOLE_HANDLE : undefined}
      contentStyle={floating ? (editing ? FLOATING_EDIT_CONTENT : FLOATING_CONTENT) : undefined}
      handleLabel="Close console"
      testID="ConsoleSheet"
    >
      <SegmentedControl variant="thumb" tone={floating ? "neutral" : "accent"} density={floating ? "compact" : "default"} trackStyle={darkGlass ? CONSOLE_TAB_TRACK : undefined} thumbStyle={darkGlass ? CONSOLE_TAB_THUMB : undefined} testID="ConsoleTabs" options={TABS} value={state.tab} onChange={(tab) => state.setTab(tab as ConsoleTab)} />
      <Sheet.ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={{ gap: 12, paddingBottom: 12 }} testID="ConsoleBody">
        {state.tab === 'testcase' ? (
          <YStack gap={floating ? (editing ? '$2' : 10) : '$3'} role="tabpanel" testID="ConsolePanel-testcase">
            <CaseChips neutral={floating} darkGlass={darkGlass} editing={editing} cases={state.cases} selectedId={selected?.id ?? null} atCap={state.atCap} onSelect={state.select} onAdd={add} onEdit={edit} onRemove={floating && editing ? state.removeCase : undefined} />
            {selected ? (
              <>
                <CaseFields inset={floating} surfaceColor={darkGlass ? CASE_CARD : undefined} entry={selected} captions={captions} focusToken={focusToken} fieldError={fieldError} onEdit={(name, text) => state.editField(selected.id, name, text)} />
                {selected.kind === 'custom' && !floating ? (
                  <Button testID="ConsoleDeleteCase" variant="outline" size="$3" icon={Trash2} pressLog="off" onPress={() => state.removeCase(selected.id)}>
                    Delete case
                  </Button>
                ) : null}
              </>
            ) : (
              <Text variant="muted" testID="ConsoleNoCases">
                {state.hasBundled ? 'Add a case or reset to the samples.' : 'This problem has no bundled cases.'}
              </Text>
            )}
          </YStack>
        ) : (
          <YStack role="tabpanel" testID="ConsolePanel-result">
            {result}
          </YStack>
        )}
      </Sheet.ScrollView>
      {footer}
    </Sheet>
  );

  return (
    <>
      {nativeHeaderBottom === undefined ? sheet : (
        <View pointerEvents="box-none" style={[StyleSheet.absoluteFill, { top: nativeHeaderBottom }]}>
          {sheet}
        </View>
      )}
      {floating && state.open && headerHeight > 0 ? (
        <FullWindowOverlay unstable_accessibilityContainerViewIsModal={false}>
          <Pressable
            testID="ConsoleNativeHeaderBackdrop"
            accessibilityRole="button"
            accessibilityLabel="Dismiss console"
            onPress={onSheetClosed}
            style={{
              position: 'absolute',
              top,
              left: 0,
              right: 0,
              height: headerHeight,
              backgroundColor: `rgba(0,0,0,${BACKDROP_OPACITY})`,
            }}
          />
        </FullWindowOverlay>
      ) : null}
    </>
  );
}
