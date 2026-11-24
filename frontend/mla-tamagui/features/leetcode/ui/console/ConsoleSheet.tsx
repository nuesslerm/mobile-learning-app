import { Button, SegmentedControl, Text } from '@lib/tamagui-design-system';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { Trash2 } from '@lib/tamagui-design-system/icons';
import { YStack } from '@lib/tamagui-design-system/primitives';
import React from 'react';

import { captionsOf } from '../../domain/console/caseFields';
import { CONSOLE_SNAP_PERCENT } from '../../domain/console/layout';
import type { Blind75Problem } from '../../domain/models';
import type { ConsoleState, ConsoleTab } from '../../state/console/useConsole';
import { CaseChips } from './CaseChips';
import { CaseFields } from './CaseFields';

const SNAP_POINTS = [CONSOLE_SNAP_PERCENT];
const TABS = [
  { value: 'testcase', label: 'Testcase' },
  { value: 'result', label: 'Run Result' },
];

type Props = {
  problem: Blind75Problem;
  console: ConsoleState;
  fieldError: { name: string; message: string } | null;
  result: React.ReactNode;
  footer: React.ReactNode;
  onSheetClosed: () => void;
};

/** The console: a sheet with no backdrop (the editor above stays usable), the Testcase and Run Result tabs, and the footer actions. */
export function ConsoleSheet({ problem, console: state, fieldError, result, footer, onSheetClosed }: Props) {
  const captions = React.useMemo(() => captionsOf(problem), [problem]);
  const [focusToken, setFocusToken] = React.useState(0);
  const { selected } = state;

  const add = () => {
    if (state.addCase()) setFocusToken((token) => token + 1);
  };
  const edit = () => {
    if (selected?.kind === 'bundled') add();
    else setFocusToken((token) => token + 1);
  };

  return (
    <Sheet open={state.open} onOpenChange={(open) => !open && onSheetClosed()} snapPoints={SNAP_POINTS} backdrop="none" handleLabel="Close console" testID="ConsoleSheet">
      <SegmentedControl variant="thumb" testID="ConsoleTabs" options={TABS} value={state.tab} onChange={(tab) => state.setTab(tab as ConsoleTab)} />
      <Sheet.ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={{ gap: 12, paddingBottom: 12 }} testID="ConsoleBody">
        {state.tab === 'testcase' ? (
          <YStack gap="$3" role="tabpanel" testID="ConsolePanel-testcase">
            <CaseChips cases={state.cases} selectedId={selected?.id ?? null} atCap={state.atCap} onSelect={state.select} onAdd={add} onEdit={edit} />
            {selected ? (
              <>
                <CaseFields entry={selected} captions={captions} focusToken={focusToken} fieldError={fieldError} onEdit={(name, text) => state.editField(selected.id, name, text)} />
                {selected.kind === 'custom' ? (
                  <Button testID="ConsoleDeleteCase" variant="outline" size="$3" icon={Trash2} pressLog="off" onPress={() => state.removeCase(selected.id)}>
                    Delete case
                  </Button>
                ) : null}
              </>
            ) : (
              <Text variant="muted" testID="ConsoleNoCases">
                This problem has no bundled cases.
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
}
