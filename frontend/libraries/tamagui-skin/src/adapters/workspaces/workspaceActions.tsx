import { useHeldTouchGuard, useWorkspaceRename, type WorkspaceActionsProps } from '@lib/skin';
import { Button, Icon, InsetGroup, ListItem, Spinner, Text, TextField } from '@lib/tamagui-design-system';
import { Bell, BellOff, Check, Circle, Columns3, GitBranch, Pencil, Pin, PinOff } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import type { LucideIcon } from 'lucide-react-native';

import { ScreenSheet } from '../sheet/ScreenSheet';

const LIST = [60];
const FORM = [70];

function Action({ icon, label, disabled, onPress }: { icon: LucideIcon; label: string; disabled: boolean; onPress: () => void }) {
  return (
    <ListItem variant="grouped" pressLog="off" accessibilityLabel={label} disabled={disabled} onPress={onPress} testID="skin-workspace-action">
      <XStack items="center" gap="$3">
        <Icon as={icon} size={17} color="$color11" />
        <Text flex={1}>{label}</Text>
      </XStack>
    </ListItem>
  );
}

function Statuses({ picker, saving, onRetry, onStatus }: { picker: NonNullable<WorkspaceActionsProps['statusPicker']>; saving: boolean; onRetry: () => void; onStatus: (id: string) => void }) {
  return (
    <>
      <Text variant="muted" textTransform="uppercase" letterSpacing={0.5} mx="$4">
        Move to Status
      </Text>
      {picker.failed ? (
        <InsetGroup testID="skin-workspace-statuses">
          <ListItem variant="grouped" pressLog="off" accessibilityLabel="Could not load statuses. Retry" onPress={onRetry} testID="skin-workspace-statuses-retry">
            <Text>Could not load statuses. Retry</Text>
          </ListItem>
        </InsetGroup>
      ) : !picker.choices ? (
        <Spinner accessibilityLabel="Loading statuses" />
      ) : (
        <InsetGroup testID="skin-workspace-statuses">
          {picker.choices.map((status) => {
            const checked = picker.selected === status.id;
            return (
              <XStack
                key={status.id}
                role="radio"
                accessible
                aria-label={status.label}
                aria-checked={checked}
                aria-disabled={saving}
                disabled={saving}
                items="center"
                gap="$3"
                px="$4"
                py="$3"
                pressStyle={{ bg: '$color3' }}
                cursor="pointer"
                onPress={() => onStatus(status.id)}
              >
                <Icon as={Circle} size={16} color="$color11" />
                <Text flex={1}>{status.label}</Text>
                {checked ? <Icon as={Check} size={18} /> : null}
              </XStack>
            );
          })}
        </InsetGroup>
      )}
    </>
  );
}

/** What a workspace offers on a long press: its actions as list rows, a form to rename it, and the statuses it can move to. */
export function WorkspaceActions(props: WorkspaceActionsProps) {
  const { renaming, start, draft, setDraft, save } = useWorkspaceRename(props);
  const guard = useHeldTouchGuard();
  return (
    <ScreenSheet onClose={props.onClose} snapPoints={renaming ? FORM : LIST} handleLabel="Dismiss" windowRef={guard} testID="skin-workspace-actions">
      <YStack gap="$3" pb="$4">
        {props.statusPicker ? (
          <Statuses picker={props.statusPicker} saving={props.saving} onRetry={props.onOpenStatuses} onStatus={props.onStatus} />
        ) : renaming ? (
          <YStack gap="$3" px="$4">
            <YStack gap="$1">
              <Text variant="large" role="heading">
                Rename Workspace
              </Text>
              {props.branch ? <Text variant="muted">Leave empty to use the branch name.</Text> : null}
            </YStack>
            <TextField
              testID="skin-workspace-name"
              aria-label="Workspace name"
              autoFocus
              selectTextOnFocus
              value={draft}
              onChangeText={setDraft}
              autoCapitalize="none"
              autoCorrect={false}
              spellCheck={false}
              returnKeyType="done"
              onSubmitEditing={save}
              placeholder={props.automaticName}
              placeholderTextColor="$color10"
            />
            <XStack gap="$3" justify="flex-end">
              <Button variant="ghost" glass={false} disabled={props.saving} pressLog="off" onPress={props.onClose} testID="skin-workspace-rename-cancel">
                Cancel
              </Button>
              <Button disabled={props.saving} pressLog="off" onPress={save} testID="skin-workspace-rename-save">
                Save
              </Button>
            </XStack>
          </YStack>
        ) : (
          <>
            <YStack px="$4" gap="$1">
              <Text variant="large" role="heading" numberOfLines={1}>
                {props.name}
              </Text>
              {props.branch ? (
                <Text variant="muted" numberOfLines={1}>
                  {props.branch}
                </Text>
              ) : null}
            </YStack>
            <InsetGroup testID="skin-workspace-actions-list">
              <Action
                icon={GitBranch}
                label="Source Control"
                disabled={props.saving}
                onPress={() => {
                  props.onClose();
                  props.onSourceControl();
                }}
              />
              <Action icon={Pencil} label="Rename" disabled={props.saving} onPress={start} />
              <Action icon={props.unread ? BellOff : Bell} label={props.unread ? 'Mark Read' : 'Mark Unread'} disabled={props.saving} onPress={props.onToggleRead} />
              <Action icon={props.pinned ? PinOff : Pin} label={props.pinned ? 'Unpin' : 'Pin'} disabled={props.saving} onPress={props.onPin} />
              <Action icon={Columns3} label="Move to Status" disabled={props.saving} onPress={props.onOpenStatuses} />
            </InsetGroup>
          </>
        )}
        {props.error ? (
          <Text role="alert" mx="$4">
            {props.error}
          </Text>
        ) : null}
      </YStack>
    </ScreenSheet>
  );
}
