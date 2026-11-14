import type { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';
import { Logger } from '@lib/logging';
import {
  Button,
  ListItem,
  SegmentedControl,
  Text,
  TextField,
  View,
} from '@lib/tamagui-design-system';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { useEffect, useMemo, useState } from 'react';

import { CODING_AGENT_TOKEN_STORAGE_KEY } from '../data/codingAgentConfig';
import {
  INTERACTION_MODE_OPTIONS,
  RUNTIME_MODE_OPTIONS,
  type InteractionMode,
  type ModelSelection,
  type RuntimeMode,
} from '../domain';
import { useThreadCommands } from '../state/useThreadCommands';
import { useComposerStore } from '../state/composerStore';
import { useGatewayStore } from '../state/gatewayStore';
import { useServerConfigStore } from '../state/serverConfigStore';
import { useThreadSettingsRequestStore } from '../state/threadSettingsRequestStore';

export function ThreadSettingsSheet(): React.ReactElement | null {
  const payload = useThreadSettingsRequestStore((state) => state.payload);
  const clear = useThreadSettingsRequestStore((state) => state.clear);
  const threadId = payload?.threadId as ThreadId | undefined;
  const open = threadId !== undefined;

  const probe = useServerConfigStore((state) => state.probe);
  const loading = useServerConfigStore((state) => state.loading);
  const load = useServerConfigStore((state) => state.load);
  const gateway = useGatewayStore((state) => state.gateway);

  const currentSelection = useComposerStore((state) =>
    threadId !== undefined ? state.modelSelection[threadId] : null
  );
  const runtimeMode = useComposerStore((state) =>
    threadId !== undefined ? state.runtimeMode[threadId] : undefined
  );
  const interactionMode = useComposerStore((state) =>
    threadId !== undefined ? state.interactionMode[threadId] : undefined
  );

  const { setModelSelection, setRuntimeMode, setInteractionMode } =
    useThreadCommands(threadId ?? ('' as ThreadId));

  const [token, setToken] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open) return;
    let live = true;
    AsyncStorage.getItem(CODING_AGENT_TOKEN_STORAGE_KEY)
      .then((value) => {
        if (live) setToken(value ?? '');
      })
      .catch((cause) => {
        if (live) setToken('');
        Logger.withError(cause).warn('coding-agent.settings.token_read_failed');
      });
    return () => {
      live = false;
    };
  }, [open]);

  useEffect(() => {
    if (!open || probe !== null || loading) return;
    if (gateway !== null) void load(gateway);
  }, [open, probe, loading, gateway, load]);

  const groups = useMemo(() => {
    if (threadId === undefined || probe === null) return [];
    const current = currentSelection ?? null;
    return useServerConfigStore.getState().modelOptions(current);
  }, [threadId, probe, currentSelection]);

  const handleSelectModel = (selection: ModelSelection) => {
    if (threadId === undefined) return;
    void setModelSelection(selection);
  };

  const handleSaveToken = async () => {
    setSaving(true);
    try {
      if (token.length === 0) {
        await AsyncStorage.removeItem(CODING_AGENT_TOKEN_STORAGE_KEY);
      } else {
        await AsyncStorage.setItem(CODING_AGENT_TOKEN_STORAGE_KEY, token);
      }
      Logger.info('coding-agent.settings.token_saved', {
        cleared: token.length === 0,
      });
    } catch (cause) {
      Logger.withError(cause).warn('coding-agent.settings.token_save_failed');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        if (!next) clear();
      }}
      testID="ThreadSettingsSheet"
    >
      <SheetHeader onClose={() => clear()} testID="ThreadSettingsSheetHeader">
        <Text variant="h4">Thread settings</Text>
      </SheetHeader>
      <Sheet.ScrollView>
        <View gap="$3" pb="$4">
          <View gap="$1">
            <Text variant="strong" testID="ThreadSettingsModelSection">
              Model
            </Text>
            {groups.map((group) => (
              <View key={group.cli} gap="$1">
                <Text
                  variant="muted"
                  size="$3"
                  testID={`ModelGroup-${group.cli}`}
                >
                  {group.cliLabel}
                  {group.isAvailable ? '' : ' (unavailable)'}
                </Text>
                {group.options.map((option) => (
                  <ListItem
                    key={option.key}
                    testID={`ModelOption-${option.cli}-${option.modelID}`}
                    showChevron={false}
                    onPress={() => handleSelectModel(option.selection)}
                  >
                    <Text>{option.label}</Text>
                    <Text variant="muted">
                      {option.isCurrent
                        ? 'Current'
                        : option.isLegacy
                          ? 'Legacy'
                          : option.subtitle}
                    </Text>
                  </ListItem>
                ))}
              </View>
            ))}
          </View>

          <View gap="$1">
            <Text variant="strong" testID="ThreadSettingsRuntimeSection">
              Runtime mode
            </Text>
            <SegmentedControl
              testID="RuntimeModeOption"
              options={RUNTIME_MODE_OPTIONS.map((option) => ({
                value: option.mode,
                label: option.label,
              }))}
              value={runtimeMode ?? RUNTIME_MODE_OPTIONS[0].mode}
              onChange={(value) => {
                void setRuntimeMode(value as RuntimeMode);
              }}
              pressLog="off"
            />
          </View>

          <View gap="$1">
            <Text variant="strong" testID="ThreadSettingsInteractionSection">
              Interaction mode
            </Text>
            <SegmentedControl
              testID="InteractionModeOption"
              options={INTERACTION_MODE_OPTIONS.map((option) => ({
                value: option.mode,
                label: option.label,
              }))}
              value={interactionMode ?? INTERACTION_MODE_OPTIONS[0].mode}
              onChange={(value) => {
                void setInteractionMode(value as InteractionMode);
              }}
              pressLog="off"
            />
          </View>

          <View gap="$1.5">
            <Text variant="strong" testID="ThreadSettingsTokenSection">
              API token
            </Text>
            <TextField
              testID="ThreadSettingsToken"
              placeholder="Masked token"
              secureTextEntry
              autoCapitalize="none"
              autoCorrect={false}
              value={token}
              onChangeText={setToken}
            />
            <Button
              variant="secondary"
              testID="ThreadSettingsTokenSave"
              disabled={saving}
              onPress={() => void handleSaveToken()}
            >
              Save token
            </Button>
          </View>
        </View>
      </Sheet.ScrollView>
    </Sheet>
  );
}
