import { Logger } from '@lib/logging';
import { useMemo } from 'react';
import { Badge, Button, Text } from '@lib/tamagui-design-system';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import {
  useCodeHistoryStore,
  type CodeSnapshot,
  type SnapshotReason,
} from '../state/codeHistoryStore';

type Props = {
  problemId: string;
  onRestore: (code: string, language: string) => void;
  onClose: () => void;
};

// What was about to happen to the buffer, phrased as what you'd have lost.
const REASON_LABEL: Record<SnapshotReason, string> = {
  format: 'Before formatting',
  run: 'Before running',
  reset: 'Before reset',
  'language-switch': 'Before language switch',
  'voice-edit': 'Before voice edit',
  'navigate-away': 'Before leaving the editor',
};

function relativeTime(then: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 60) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

function firstMeaningfulLine(code: string): string {
  const line = code
    .split('\n')
    .map((l) => l.trim())
    .find((l) => l.length > 0 && !l.startsWith('#') && !l.startsWith('//'));
  return line ?? '(empty)';
}

function SnapshotRow({
  snapshot,
  now,
  onRestore,
}: {
  snapshot: CodeSnapshot;
  now: number;
  onRestore: (snapshot: CodeSnapshot) => void;
}) {
  return (
    <YStack
      testID="CodeHistoryRow"
      gap="$1.5"
      py="$3"
      borderBottomWidth={1}
      borderColor="$borderColor"
    >
      <XStack items="center" justify="space-between" gap="$2">
        <XStack items="center" gap="$2" flex={1}>
          <Text testID="CodeHistoryRowReason" numberOfLines={1}>
            {REASON_LABEL[snapshot.reason]}
          </Text>
          <Badge>{snapshot.language}</Badge>
        </XStack>
        <Text variant="muted" testID="CodeHistoryRowAge">
          {relativeTime(snapshot.createdAt, now)}
        </Text>
      </XStack>

      <Text variant="muted" numberOfLines={1} testID="CodeHistoryRowPreview">
        {firstMeaningfulLine(snapshot.code)}
      </Text>

      <XStack items="center" justify="space-between" gap="$2">
        <Text variant="muted">{snapshot.code.length} chars</Text>
        <Button
          variant="secondary"
          size="$2"
          testID="CodeHistoryRestore"
          pressLog="off"
          onPress={() => onRestore(snapshot)}
        >
          Restore
        </Button>
      </XStack>
    </YStack>
  );
}

/**
 * Version history for one problem's code buffer.
 *
 * Read-only plus restore: there is no delete-one, because the interesting
 * failure here is losing work, not accumulating it, and a per-row delete is one
 * more way to lose it.
 */
export function CodeHistorySheet({ problemId, onRestore, onClose }: Props) {
  // Select the stable array and narrow it OUTSIDE the selector. Filtering
  // inside returns a fresh array on every render, and zustand compares
  // selector results by reference -- which is an infinite render loop
  // (React #185, "maximum update depth exceeded"), not a subtle inefficiency.
  const allSnapshots = useCodeHistoryStore((s) => s.snapshots);
  const ordered = useMemo(
    () =>
      allSnapshots
        .filter((snap) => snap.problemId === problemId)
        .sort((a, b) => b.createdAt - a.createdAt),
    [allSnapshots, problemId]
  );
  const now = Date.now();

  return (
    <Sheet
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      testID="CodeHistorySheet"
    >
      <SheetHeader testID="CodeHistorySheetHeader" onClose={onClose}>
        <Text variant="large">Version history</Text>
        <Text variant="muted">
          {ordered.length > 0
            ? `${ordered.length} saved before a change replaced the buffer`
            : 'Nothing saved yet'}
        </Text>
      </SheetHeader>

      {ordered.length === 0 ? (
        <YStack py="$6" items="center" gap="$2" testID="CodeHistoryEmpty">
          <Text variant="muted">
            Versions are saved automatically before Format, Reset, a
          </Text>
          <Text variant="muted">language switch, or a voice edit.</Text>
        </YStack>
      ) : (
        <Sheet.ScrollView testID="CodeHistoryScroll">
          {ordered.map((snapshot) => (
            <SnapshotRow
              key={snapshot.id}
              snapshot={snapshot}
              now={now}
              onRestore={(chosen) => {
                Logger.info('leetcode.history.restore', {
                  problemId,
                  snapshotId: chosen.id,
                  reason: chosen.reason,
                  length: chosen.code.length,
                });
                onRestore(chosen.code, chosen.language);
                onClose();
              }}
            />
          ))}
        </Sheet.ScrollView>
      )}
    </Sheet>
  );
}
