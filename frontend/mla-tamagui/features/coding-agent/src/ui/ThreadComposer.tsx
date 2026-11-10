import { useCallback } from 'react';
import {
  INTERACTION_MODE_OPTIONS,
  RUNTIME_MODE_OPTIONS,
  type InteractionMode,
  type RuntimeMode,
} from '../domain';
import type { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';
import {
  Badge,
  Chip,
  ComposerBase,
  SegmentedControl,
  View,
} from '@lib/tamagui-design-system';

import { useThreadCommands } from '../integration/useThreadCommands';
import {
  EMPTY_QUEUE,
  getDefaultInteractionMode,
  getDefaultRuntimeMode,
  useComposerStore,
} from '../state/composerStore';
import { useGatewayStore } from '../state/gatewayStore';
import { useThreadDetailStore } from '../state/threadDetailStore';
import { useThreadSettingsRequestStore } from '../state/threadSettingsRequestStore';

function runtimeModeLabel(mode: RuntimeMode): string {
  return (
    RUNTIME_MODE_OPTIONS.find((option) => option.mode === mode)?.label ?? mode
  );
}

export function ThreadComposer({
  threadId,
}: {
  threadId: ThreadId;
}): React.ReactElement {
  const draft = useComposerStore((state) => state.drafts[threadId] ?? '');
  const setDraft = useComposerStore((state) => state.setDraft);
  const runtimeMode =
    useComposerStore((state) => state.runtimeMode[threadId]) ??
    getDefaultRuntimeMode(threadId);
  const interactionMode =
    useComposerStore((state) => state.interactionMode[threadId]) ??
    getDefaultInteractionMode(threadId);
  const modelSelection = useComposerStore(
    (state) => state.modelSelection[threadId],
  );
  const queued = useComposerStore((state) => state.queued[threadId] ?? EMPTY_QUEUE);

  const { startTurn, interrupt, setInteractionMode } =
    useThreadCommands(threadId);
  const status = useGatewayStore((state) => state.status);

  const thread = useThreadDetailStore((state) => state.threads[threadId]);
  const isRunning =
    thread?.latestTurn?.state === 'running' ||
    thread?.session?.status === 'starting' ||
    thread?.session?.status === 'running';

  const disabled = status !== 'open' || draft.trim().length === 0;
  const modelLabel = modelSelection?.modelID ?? modelSelection?.cli ?? 'Model';

  const openSettings = useCallback(() => {
    useThreadSettingsRequestStore.getState().request({ threadId });
  }, [threadId]);

  const handleSend = useCallback(() => {
    const text = draft.trim();
    if (text.length === 0) return;
    setDraft(threadId, '');
    void startTurn(text);
  }, [draft, setDraft, startTurn, threadId]);

  const handleStop = useCallback(() => {
    void interrupt();
  }, [interrupt]);

  return (
    <ComposerBase
      testID="ThreadComposer"
      value={draft}
      onChangeText={(text) => setDraft(threadId, text)}
      placeholder={
        interactionMode === 'plan' ? 'Describe what to plan…' : 'Message…'
      }
      isRunning={isRunning}
      disabled={disabled}
      onSend={handleSend}
      onStop={handleStop}
      chips={
        <View flexDirection="row" items="center" gap="$1.5" flexWrap="wrap">
          <Chip
            testID="ModelChip"
            label={modelLabel}
            size="sm"
            onPress={openSettings}
          />
          <Chip
            testID="RuntimeModeChip"
            label={runtimeModeLabel(runtimeMode)}
            size="sm"
            onPress={openSettings}
          />
          <SegmentedControl
            testID="InteractionModeSegment"
            options={INTERACTION_MODE_OPTIONS.map((option) => ({
              value: option.mode,
              label: option.label,
            }))}
            value={interactionMode}
            onChange={(value) => {
              void setInteractionMode(value as InteractionMode);
            }}
            pressLog="off"
          />
          {queued.length > 0 ? (
            <Badge variant="secondary" testID="ComposerQueuedBadge">
              {queued.length}
            </Badge>
          ) : null}
        </View>
      }
    />
  );
}
