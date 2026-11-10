import * as React from 'react';
import {
  Platform,
  type NativeSyntheticEvent,
  type TextInputContentSizeChangeEventData,
} from 'react-native';

import { ArrowUp, Square } from '../icons';
import { XStack, YStack } from '../primitives';
import { IconButton } from '../ui/icon-button';
import { Input } from '../ui/text-field';

const MIN_HEIGHT = 38;
const MAX_HEIGHT = 140;

/**
 * Tracks a multiline input's content height, clamped to [min, max]. Value-aware:
 * when the input is empty it pins to `min`, because react-native-web's multiline
 * TextInput over-reports `contentSize.height` for an empty field (which made the
 * card render tall and empty). While typing, it grows up to `max` then scrolls.
 */
export function useAutoGrowInput(
  value: string,
  min = MIN_HEIGHT,
  max = MAX_HEIGHT
) {
  const [measured, setMeasured] = React.useState(min);
  const onContentSizeChange = React.useCallback(
    (e: NativeSyntheticEvent<TextInputContentSizeChangeEventData>) => {
      setMeasured(
        Math.min(max, Math.max(min, e.nativeEvent.contentSize.height))
      );
    },
    [min, max]
  );
  const height = value.length === 0 ? min : measured;
  return { height, atMax: height >= max, onContentSizeChange };
}

export type ComposerBaseProps = {
  value: string;
  onChangeText: (text: string) => void;
  onSend: () => void;
  onStop?: () => void;
  isRunning?: boolean;
  disabled?: boolean;
  placeholder?: string;
  /** Row rendered above the input (e.g. attachment chips). */
  chips?: React.ReactNode;
  /** Left side of the control row (e.g. a "+" attach button). */
  leading?: React.ReactNode;
  /** Right side of the control row, before send (e.g. a mic button). */
  trailing?: React.ReactNode;
  testID: string;
};

/**
 * A Claude-style message input rendered as a single rounded card, meant to sit
 * IN the document flow (a sibling below a flex-1 scroll area), not absolutely
 * positioned. Presentational + controlled, with no chat coupling, so it stays
 * reusable across features.
 *
 * testIDs derive from the `testID` prefix: `${testID}Input`,
 * `${testID}SendButton`, `${testID}StopButton`.
 */
export function ComposerBase({
  value,
  onChangeText,
  onSend,
  onStop,
  isRunning = false,
  disabled = false,
  placeholder = 'Message…',
  chips,
  leading,
  trailing,
  testID,
}: ComposerBaseProps): React.ReactElement {
  const isWeb = Platform.OS === 'web';
  const { height, atMax, onContentSizeChange } = useAutoGrowInput(value);

  return (
    <YStack
      testID={testID}
      bg="$color2"
      rounded="$6"
      borderWidth={1}
      borderColor="$borderColor"
      px="$3"
      pt="$2"
      pb="$2"
      gap="$1.5"
    >
      {chips}
      <ComposerInput
        testID={testID}
        value={value}
        onChangeText={onChangeText}
        placeholder={placeholder}
        isWeb={isWeb}
        height={height}
        atMax={atMax}
        onContentSizeChange={onContentSizeChange}
      />
      <ComposerControlRow
        testID={testID}
        leading={leading}
        trailing={trailing}
        isRunning={isRunning}
        disabled={disabled}
        onSend={onSend}
        onStop={onStop}
      />
    </YStack>
  );
}

function ComposerInput({
  testID,
  value,
  onChangeText,
  placeholder,
  isWeb,
  height,
  atMax,
  onContentSizeChange,
}: {
  testID: string;
  value: string;
  onChangeText: (text: string) => void;
  placeholder: string;
  isWeb: boolean;
  height: number;
  atMax: boolean;
  onContentSizeChange: (
    e: NativeSyntheticEvent<TextInputContentSizeChangeEventData>
  ) => void;
}): React.ReactElement {
  return (
    <Input
      testID={`${testID}Input`}
      value={value}
      onChangeText={onChangeText}
      placeholder={placeholder}
      multiline
      // Enter inserts a newline (no submit-on-Enter); send is button-only.
      // `outlineWidth: 0` removes the browser focus ring (DS idiom).
      style={
        isWeb
          ? { height, lineHeight: 20, outlineWidth: 0 }
          : {
              minHeight: MIN_HEIGHT,
              maxHeight: MAX_HEIGHT,
              lineHeight: 20,
              outlineWidth: 0,
            }
      }
      {...(isWeb
        ? ({
            onContentSizeChange,
            scrollEnabled: atMax,
          } as Record<string, unknown>)
        : {})}
    />
  );
}

function ComposerControlRow({
  testID,
  leading,
  trailing,
  isRunning,
  disabled,
  onSend,
  onStop,
}: {
  testID: string;
  leading?: React.ReactNode;
  trailing?: React.ReactNode;
  isRunning: boolean;
  disabled: boolean;
  onSend: () => void;
  onStop?: () => void;
}): React.ReactElement {
  return (
    <XStack items="center" gap="$1.5">
      {leading}
      <YStack flex={1} />
      {trailing}
      {isRunning ? (
        <IconButton
          icon={Square}
          testID={`${testID}StopButton`}
          aria-label="Stop generating"
          variant="destructive"
          onPress={() => onStop?.()}
        />
      ) : (
        <IconButton
          icon={ArrowUp}
          testID={`${testID}SendButton`}
          aria-label="Send message"
          variant="default"
          disabled={disabled}
          onPress={onSend}
        />
      )}
    </XStack>
  );
}
