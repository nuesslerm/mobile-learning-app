import { Text } from '@lib/tamagui-design-system';
import { YStack, useTheme } from '@lib/tamagui-design-system/primitives';
import React from 'react';
import { TextInput } from 'react-native';

import type { CaseFieldCaption } from '../../domain/console/caseFields';
import type { ConsoleCase } from '../../domain/console/testcases';
import { INSET_VALUE_SURFACE, ValueCard } from './ValueCard';

type Props = {
  entry: ConsoleCase;
  captions: readonly CaseFieldCaption[];
  /** Changes to a new value when the first field of a custom case should take the focus. */
  focusToken: number;
  fieldError: { name: string; message: string } | null;
  onEdit: (name: string, text: string) => void;
  inset?: boolean;
};

/** One field per parameter: a read-only card for a bundled case, an editable JSON text for a custom one. */
export function CaseFields({ entry, captions, focusToken, fieldError, onEdit, inset = false }: Props) {
  const theme = useTheme();
  const first = React.useRef<React.ElementRef<typeof TextInput>>(null);
  React.useEffect(() => {
    if (focusToken > 0 && entry.kind === 'custom') first.current?.focus();
  }, [focusToken, entry.kind, entry.id]);

  return (
    <YStack gap={inset ? "$2" : "$3"} testID="ConsoleFields">
      {captions.map(({ name, caption }, index) =>
        entry.kind === 'bundled' ? (
          <ValueCard key={`${entry.id}:${name}`} id={`ConsoleField-${name}`} caption={inset ? name : caption} inset={inset} value={entry.fields[name] ?? ''} />
        ) : (
          <YStack key={`${entry.id}:${name}`} gap="$1" {...(inset ? INSET_VALUE_SURFACE : {})}>
            <Text variant="muted" fontSize={inset ? 11 : undefined} testID={`ConsoleField-${name}-Caption`}>
              {inset ? name : caption}
            </Text>
            <TextInput
              ref={index === 0 ? first : undefined}
              testID={`ConsoleField-${name}`}
              value={entry.fields[name] ?? ''}
              onChangeText={(text) => onEdit(name, text)}
              multiline
              autoCorrect={false}
              autoCapitalize="none"
              spellCheck={false}
              accessibilityLabel={caption}
              style={{
                color: theme.color?.val,
                backgroundColor: inset ? 'transparent' : theme.color3?.val,
                borderRadius: 8,
                paddingHorizontal: inset ? 0 : 12,
                paddingVertical: inset ? 0 : 10,
                fontFamily: 'Menlo, Courier, monospace',
                fontSize: 13,
                textAlignVertical: 'top',
              }}
            />
            {fieldError?.name === name ? (
              <Text variant="small" color="$red10" role="alert" testID="ConsoleFieldError">
                {fieldError.message}
              </Text>
            ) : null}
          </YStack>
        )
      )}
    </YStack>
  );
}
