import type { LogEntry, LogLevel, PersistedLogEntry } from '@lib/logging';
import { liveTail } from '@lib/logging/liveTail';
import { Button, Text, type TextProps } from '@lib/tamagui-design-system';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import { useCallback, useEffect, useState } from 'react';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { clear, queryPage } from '../lib/logPersistence';

const LEVEL_COLOR: Record<LogLevel, TextProps['color']> = {
  debug: '$color9',
  log: '$color11',
  info: '$blue10',
  warn: '$yellow10',
  error: '$red10',
};

function formatTime(ts: number): string {
  const d = new Date(ts);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}:${String(d.getSeconds()).padStart(2, '0')}`;
}

/**
 * One log line.
 *
 * App #1 renders each of these as a `ListItem` -- a bordered, rounded CARD per
 * line -- with four different hardcoded font sizes (12.5, 11.5, 11, 10), a
 * `DOT_W = 18` constant used as `gap: DOT_W - 6`, a magic `paddingTop: 3`, and
 * the timestamp on its own line at `opacity-50`. That is what makes it read as
 * loose and messy.
 *
 * A log stream wants a dense, flush, hairline-separated list: time and level
 * on the same line as the message, one size from the scale, no card.
 */
function LogRow({
  entry,
}: {
  entry: LogEntry | PersistedLogEntry;
}) {
  return (
    <XStack gap="$2" py="$1.5" borderBottomWidth={1} borderColor="$borderColor">
      <Text variant="code" color="$color10" testID="DebugLogTime">
        {formatTime(entry.ts)}
      </Text>
      <Text
        variant="code"
        color={LEVEL_COLOR[entry.level]}
        width={44}
        testID="DebugLogLevel"
      >
        {entry.level}
      </Text>
      <Text variant="code" flex={1} numberOfLines={2} testID="DebugLogText">
        {entry.source ? `[${entry.source}] ` : ''}
        {entry.text}
      </Text>
    </XStack>
  );
}

export function DebugLogSheet({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  // Follows the throttled emits, not getEntries(): the snapshot also moves between emits, and a
  // console.error captured mid-render would make useSyncExternalStore re-render forever.
  const [liveEntries, setLiveEntries] = useState(() => liveTail.getEntries());
  useEffect(() => liveTail.subscribe((snapshot) => setLiveEntries(snapshot)), []);

  const [persistedEntries, setPersistedEntries] = useState<
    PersistedLogEntry[]
  >([]);
  const [nextBeforeId, setNextBeforeId] = useState<number | undefined>();

  const fetchPersistedPage = useCallback(async (beforeId?: number) => {
    const page = await queryPage({ limit: 50, beforeId });
    setPersistedEntries(page);
    const lastEntry = page[page.length - 1];
    setNextBeforeId(lastEntry?.dbId);
  }, []);

  const loadMoreOlder = useCallback(async () => {
    if (nextBeforeId === undefined) return;
    const page = await queryPage({ limit: 50, beforeId: nextBeforeId });
    setPersistedEntries((prev) => [...prev, ...page]);
    const lastEntry = page[page.length - 1];
    setNextBeforeId(lastEntry?.dbId);
  }, [nextBeforeId]);

  const handleClear = useCallback(async () => {
    await clear();
    setPersistedEntries([]);
    setNextBeforeId(undefined);
    liveTail.clear();
  }, []);

  useEffect(() => {
    if (open) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      void fetchPersistedPage();
    }
  }, [open, fetchPersistedPage]);

  const allEntries = [...persistedEntries, ...liveEntries].slice().reverse();

  return (
    <Sheet open={open} onOpenChange={onOpenChange} testID="DebugLogSheet">
      <SheetHeader
        testID="DebugLogSheetHeader"
        onClose={() => onOpenChange(false)}
      >
        <XStack flex={1} gap="$2" items="center">
          <YStack flex={1}>
            <Text variant="h4" testID="DebugLogSheetTitle">
              Logs
            </Text>
            <Text variant="muted" testID="DebugLogSheetCount">
              {allEntries.length} entries
            </Text>
          </YStack>
          <Button
            size="$2"
            onPress={() => void handleClear()}
            testID="DebugLogSheetClearButton"
          >
            Clear
          </Button>
        </XStack>
      </SheetHeader>
      <Sheet.ScrollView testID="DebugLogSheetScroll">
        <YStack testID="DebugLogSheetList">
          {allEntries.length === 0 ? (
            <Text variant="muted" py="$3" testID="DebugLogSheetEmpty">
              Nothing logged yet.
            </Text>
          ) : (
            <>
              {nextBeforeId !== undefined && (
                <Button
                  size="$2"
                  onPress={() => void loadMoreOlder()}
                  testID="DebugLogSheetLoadMore"
                >
                  Load older
                </Button>
              )}
              {allEntries.map((entry) => (
                <LogRow key={"id" in entry ? `live-${entry.id}` : `db-${entry.dbId}`} entry={entry} />
              ))}
            </>
          )}
        </YStack>
      </Sheet.ScrollView>
    </Sheet>
  );
}
