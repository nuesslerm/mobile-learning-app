import { IconButton, Text } from '@lib/tamagui-design-system';
import { Mic, Send, X } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';

import { AnimatedWaveform } from './AnimatedWaveform';

export type VoiceRecordingToolbarProps = {
  isRecording: boolean;
  /** Elapsed recording time in milliseconds. */
  durationMs: number;
  /** Real mic-level samples (0-1) driving the waveform -- see useVoiceRecording. */
  amplitudes?: number[];
  onCancel: () => void;
  onSend: () => void | Promise<void>;
  testID?: string;
};

const TOOLBAR_MIN_HEIGHT = 72;

function formatDuration(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
}

/**
 * Pill toolbar floating above the keyboard while a voice edit is captured.
 * Collapsed it is a prompt; recording it is cancel / waveform + timer / send.
 */
export function VoiceRecordingToolbar({
  isRecording,
  durationMs,
  amplitudes,
  onCancel,
  onSend,
  testID = 'VoiceRecordingToolbar',
}: VoiceRecordingToolbarProps) {
  if (!isRecording) {
    return (
      <XStack
        testID={`${testID}-Collapsed`}
        role="button"
        aria-label="Start voice edit"
        items="center"
        gap="$2"
        px="$4"
        height={TOOLBAR_MIN_HEIGHT}
        rounded="$5"
        borderWidth={1}
        borderColor="$borderColor"
        bg="$color2"
        pressStyle={{ opacity: 0.7 }}
        cursor="pointer"
        onPress={() => void onSend()}
      >
        <IconButton
          testID={`${testID}-Mic`}
          icon={Mic}
          variant="default"
          aria-label="Start voice edit"
          onPress={() => void onSend()}
        />
        <Text variant="muted" flex={1} testID={`${testID}-Prompt`}>
          Tap to dictate an edit
        </Text>
      </XStack>
    );
  }

  return (
    <XStack
      testID={testID}
      role="toolbar"
      aria-label="Voice recording toolbar"
      items="center"
      justify="space-between"
      gap="$3"
      px="$3"
      height={TOOLBAR_MIN_HEIGHT}
      rounded="$5"
      borderWidth={1}
      borderColor="$borderColor"
      bg="$color2"
    >
      <IconButton
        testID={`${testID}-Cancel`}
        icon={X}
        variant="ghost"
        aria-label="Cancel recording"
        onPress={onCancel}
      />
      <XStack
        flex={1}
        items="center"
        justify="center"
        gap="$3"
        testID={`${testID}-Center`}
      >
        <AnimatedWaveform
          testID={`${testID}-Waveform`}
          isRecording
          amplitudes={amplitudes}
        />
        <Text variant="code" testID={`${testID}-Duration`}>
          {formatDuration(durationMs)}
        </Text>
      </XStack>
      <IconButton
        testID={`${testID}-Send`}
        icon={Send}
        variant="default"
        aria-label="Send voice edit"
        onPress={() => void onSend()}
      />
    </XStack>
  );
}
