import { Logger } from '@lib/logging';
import { Badge, IconButton, ListItem, Text } from '@lib/tamagui-design-system';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import type { EditAlternative } from '@lib/voice-edit-dsl';
import { X } from '@lib/tamagui-design-system/icons';
import { useCallback } from 'react';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { compactVoiceError } from '../domain/voiceEditIdentity';

export type AlternativePickerProps = {
  transcript: string;
  alternatives: EditAlternative[];
  onSelect: (alternative: EditAlternative) => void;
  onDismiss: () => void;
  testID?: string;
};

function confidenceVariant(pct: number, invalid: boolean) {
  if (invalid) return 'destructive' as const;
  if (pct >= 80) return 'success' as const;
  if (pct >= 50) return 'secondary' as const;
  return 'warning' as const;
}

function AlternativeRow({
  alternative,
  onPress,
  testID,
}: {
  alternative: EditAlternative;
  onPress: (alt: EditAlternative) => void;
  testID: string;
}) {
  const isInvalid = alternative.valid === false;
  const confidencePct = Math.round(
    Math.min(1, Math.max(0, alternative.confidence)) * 100
  );

  return (
    <ListItem
      testID={testID}
      pressLog="off"
      onPress={() => onPress(alternative)}
    >
      <XStack items="center" gap="$3">
        <YStack flex={1} gap="$1" opacity={isInvalid ? 0.5 : 1}>
          <Text variant="strong" testID={`${testID}-Label`}>
            {alternative.label}
          </Text>
          {alternative.summary ? (
            <Text variant="muted" testID={`${testID}-Summary`}>
              {alternative.summary}
            </Text>
          ) : null}
          {isInvalid && alternative.invalidReason ? (
            <Text
              variant="small"
              color="$red10"
              testID={`${testID}-InvalidReason`}
            >
              {alternative.invalidReason}
            </Text>
          ) : null}
        </YStack>
        <Badge
          variant={confidenceVariant(confidencePct, isInvalid)}
          testID={`${testID}-Badge`}
        >
          {`${confidencePct}%`}
        </Badge>
      </XStack>
    </ListItem>
  );
}

/**
 * Bottom sheet asking the user to disambiguate between competing voice-edit
 * interpretations, shown when the pipeline returns two or more alternatives.
 *
 * App #1's version drives a nine-part compound Sheet whose `Sheet.Header`
 * takes prefix/center/trailing slots plus `showHandle`/`tinted` flags. This
 * one is Root/Header/ScrollView, which is all those slots were expressing.
 */
export function AlternativePicker({
  transcript,
  alternatives,
  onSelect,
  onDismiss,
  testID = 'AlternativePicker',
}: AlternativePickerProps) {
  const handleSelect = useCallback(
    (alt: EditAlternative) => {
      if (alt.valid === false) {
        Logger.info('alternative.select.blocked', {
          testID,
          alternativeId: alt.id,
          invalidReason: alt.invalidReason
            ? compactVoiceError(alt.invalidReason)
            : undefined,
        });
        return;
      }
      Logger.info('alternative.select', {
        testID,
        alternativeId: alt.id,
        confidence: alt.confidence,
      });
      onSelect(alt);
      onDismiss();
    },
    [onSelect, onDismiss, testID]
  );

  const handleClose = useCallback(() => {
    Logger.info('alternative.dismiss', { testID });
    onDismiss();
  }, [onDismiss, testID]);

  return (
    <Sheet
      open
      onOpenChange={(next) => {
        if (!next) handleClose();
      }}
      snapPoints={[90, 60]}
      testID={testID}
    >
      <SheetHeader testID={`${testID}-Header`}>
        <XStack items="center" gap="$3">
          <Text
            flex={1}
            variant="muted"
            numberOfLines={2}
            testID={`${testID}-Transcript`}
          >
            {`“${transcript}”`}
          </Text>
          <IconButton
            testID={`${testID}-Close`}
            icon={X}
            variant="ghost"
            aria-label="Close alternative picker"
            onPress={handleClose}
          />
        </XStack>
      </SheetHeader>
      <Sheet.ScrollView testID={`${testID}-Content`}>
        <YStack gap="$2.5" pb="$6">
          {alternatives.length === 0 ? (
            <Text
              variant="muted"
              py="$10"
              text="center"
              testID={`${testID}-Empty`}
            >
              No alternative interpretations were returned.
            </Text>
          ) : (
            alternatives.map((alt, idx) => (
              <AlternativeRow
                key={alt.id}
                alternative={alt}
                onPress={handleSelect}
                testID={`${testID}-Row-${idx}`}
              />
            ))
          )}
        </YStack>
      </Sheet.ScrollView>
    </Sheet>
  );
}
