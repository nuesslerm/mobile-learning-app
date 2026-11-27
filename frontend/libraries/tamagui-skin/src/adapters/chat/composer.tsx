import {
  ADD_CHOICES,
  useComposerEditing,
  useComposerSuggestionRows,
  useSkin,
  type ComposerAddButtonProps,
  type ComposerProps,
  type ComposerSendButtonProps,
  type ComposerSuggestionListProps,
} from '@lib/skin';
import { Icon, IconButton, InsetGroup, ListItem, Spinner, Text } from '@lib/tamagui-design-system';
import { ArrowUp, AtSign, FileText, Package, Plus, Terminal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack, useTheme } from '@lib/tamagui-design-system/primitives';
import { useState } from 'react';
import { Keyboard, ScrollView, TextInput, type TextInputInstance } from 'react-native';

import { ScreenSheet } from '../sheet/ScreenSheet';

export function ComposerSendButton({ label, canSend, sending = false, onPress, onLongPress }: ComposerSendButtonProps) {
  const enabled = canSend && !sending;
  return (
    <XStack
      role="button"
      aria-label={label}
      aria-disabled={!enabled}
      aria-busy={sending}
      width={36}
      height={36}
      rounded={18}
      items="center"
      justify="center"
      bg={canSend ? '$blue9' : '$color4'}
      pressStyle={{ opacity: 0.7 }}
      cursor="pointer"
      disabled={!enabled}
      onPress={onPress}
      onLongPress={onLongPress}
    >
      {sending ? <Spinner colorToken="color1" /> : <Icon as={ArrowUp} size={19} color={canSend ? '$color1' : '$color10'} />}
    </XStack>
  );
}

const ADD_ICONS = { file: AtSign, skill: Package, command: Terminal } as const;

export function ComposerAddButton({ onSelect, onDismiss }: ComposerAddButtonProps) {
  const [open, setOpen] = useState(false);
  const close = () => {
    setOpen(false);
    onDismiss();
  };
  return (
    <>
      <IconButton
        icon={Plus}
        variant="secondary"
        glass={false}
        size="$3"
        accessibilityLabel="Add to message"
        pressLog="off"
        onPress={() => {
          Keyboard.dismiss();
          setOpen(true);
        }}
        testID="skin-composer-add"
      />
      {open ? (
        <ScreenSheet onClose={close} snapPoints={[35]} handleLabel="Close" testID="skin-composer-add-sheet">
          <YStack pb="$4">
            <InsetGroup testID="skin-composer-add-choices">
              {ADD_CHOICES.map(({ kind, label }) => (
                <ListItem
                  key={kind}
                  variant="grouped"
                  pressLog="off"
                  accessibilityLabel={label}
                  onPress={() => {
                    onSelect(kind);
                    close();
                  }}
                  testID={`skin-composer-add-${kind}`}
                >
                  <XStack items="center" gap="$3">
                    <Icon as={ADD_ICONS[kind]} size={16} color="$color11" />
                    <Text>{label}</Text>
                  </XStack>
                </ListItem>
              ))}
            </InsetGroup>
          </YStack>
        </ScreenSheet>
      ) : null}
    </>
  );
}

const SUGGESTION_ICONS = { file: FileText, skill: Package, command: Terminal } as const;

export function ComposerSuggestionList({ source, sigil, query, kind, onPick }: ComposerSuggestionListProps) {
  const { error, empty, retry, groups } = useComposerSuggestionRows(source, sigil, query, kind);
  if (empty) return null;
  return (
    <YStack mx="$3" mb="$1" rounded="$4" bg="$color2" borderWidth={1} borderColor="$borderColor" overflow="hidden">
      <ScrollView style={{ maxHeight: 300 }} keyboardShouldPersistTaps="always">
        {error ? (
          <ListItem variant="grouped" pressLog="off" accessibilityLabel="Couldn’t load suggestions. Try again" onPress={retry} testID="skin-composer-suggestions-retry">
            <Text>Couldn’t load suggestions. Try again</Text>
          </ListItem>
        ) : null}
        {groups.map(({ group, heading, rows }) => (
          <YStack key={group}>
            <Text variant="muted" textTransform="uppercase" letterSpacing={0.5} px="$4" pt="$2">
              {heading}
            </Text>
            {rows.map((row) => (
              <ListItem key={row.token} variant="grouped" pressLog="off" accessibilityLabel={row.token} onPress={() => onPick(row)} testID="skin-composer-suggestion">
                <XStack gap="$3">
                  <Icon as={SUGGESTION_ICONS[group]} size={16} color="$color11" />
                  <YStack flex={1} gap="$1">
                    <XStack items="baseline" gap="$2">
                      <Text variant="code" numberOfLines={1} shrink={1}>
                        {row.token}
                      </Text>
                      {row.hint ? (
                        <Text variant="muted" numberOfLines={1}>
                          {row.hint}
                        </Text>
                      ) : null}
                    </XStack>
                    {row.description ? (
                      <Text variant="muted" numberOfLines={2}>
                        {row.description}
                      </Text>
                    ) : null}
                  </YStack>
                </XStack>
              </ListItem>
            ))}
          </YStack>
        ))}
      </ScrollView>
    </YStack>
  );
}

/** The message field in the shape of the app's composer: one rounded surface, the text above its controls. */
export function Composer({ suggestions, notice, onPickSuggestion, value, placeholder, sendLabel, canSend, sending, leadingControls, trailingControls, onChangeText, onSend }: ComposerProps) {
  const { ComposerSendButton: Send, ComposerAddButton: Add, ComposerSuggestionList: Suggestions } = useSkin();
  const theme = useTheme();
  const { field, selection, trigger, kind, selectionChanged, textChanged, open, pick, addDismissed } =
    useComposerEditing<TextInputInstance>({ value, notice, onChangeText, onPickSuggestion });
  return (
    <>
      {notice ? (
        <XStack justify="center" px="$3" pb="$1" role="alert" aria-live="assertive">
          <Text variant="small" color="$red10" shrink={1}>
            {notice}
          </Text>
        </XStack>
      ) : null}
      {suggestions && trigger ? <Suggestions source={suggestions} sigil={trigger.sigil} query={trigger.query} kind={kind} onPick={pick} /> : null}
      <YStack px="$3" pt="$1.5" pb="$2">
        <YStack bg="$color2" rounded="$6" borderWidth={1} borderColor="$borderColor" px="$3" pt="$2" pb="$2" gap="$1.5">
          {/* the field is the platform's own: the caret and the selection it reports drive the suggestions */}
          <TextInput
            ref={field}
            selection={selection}
            onSelectionChange={({ nativeEvent }) => selectionChanged(nativeEvent.selection)}
            value={value}
            onChangeText={textChanged}
            placeholder={placeholder}
            placeholderTextColor={theme.color10?.val}
            selectionColor={theme.blue9?.val}
            multiline
            textAlignVertical="top"
            style={{ minHeight: 38, maxHeight: 140, fontSize: 16, lineHeight: 22, color: theme.color?.val, outlineWidth: 0 }}
          />
          <XStack items="center" gap="$1.5">
            {suggestions ? <Add onSelect={open} onDismiss={addDismissed} /> : null}
            {leadingControls}
            <YStack flex={1} />
            {trailingControls}
            <Send label={sendLabel} canSend={canSend} sending={sending} onPress={onSend} />
          </XStack>
        </YStack>
      </YStack>
    </>
  );
}
