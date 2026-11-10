import { Button, IconButton, Spinner, Text } from '@lib/tamagui-design-system';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import { Pause, Play, RotateCcw, Send, X } from '@lib/tamagui-design-system/icons';
import { TextInput } from 'react-native';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { useVoicePlayback } from '../hooks/useVoicePlayback';

export type VoiceReviewSheetProps = {
  /**
   * 'voice' (default): the transcript of a recording, with playback and
   * re-record. 'text': the user types the instruction here instead of
   * speaking it -- no recording exists, so neither control is shown. Send
   * goes to the same plan endpoint either way.
   */
  mode?: 'voice' | 'text';
  /** Local URI of the just-recorded clip -- played back so the user can check what they said. */
  recordingUri: string | null;
  /** null while whisper is still transcribing. */
  transcript: string | null;
  onTranscriptChange: (text: string) => void;
  onReRecord: () => void;
  onCancel: () => void;
  onSend: () => void;
  testID?: string;
};

function formatSeconds(sec: number): string {
  const total = Math.max(0, Math.floor(sec));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
}

function PlaybackRow({
  recordingUri,
  testID,
}: {
  recordingUri: string | null;
  testID: string;
}) {
  const { playing, currentTime, duration, toggle } = useVoicePlayback(recordingUri);

  return (
    <XStack
      items="center"
      gap="$3"
      px="$3"
      py="$2"
      rounded="$4"
      borderWidth={1}
      borderColor="$borderColor"
      testID={testID}
    >
      <IconButton
        testID={`${testID}-Toggle`}
        icon={playing ? Pause : Play}
        variant="default"
        aria-label={playing ? 'Pause playback' : 'Play recording'}
        onPress={toggle}
      />
      <Text variant="code" testID={`${testID}-Time`}>
        {`${formatSeconds(currentTime)} / ${formatSeconds(duration)}`}
      </Text>
    </XStack>
  );
}

/**
 * Shown after stopping a voice-edit recording (or, in text mode, as soon as
 * the user asks to type an edit), BEFORE the LLM is engaged:
 * lets the user hear the raw clip back and read/edit whisper's transcript,
 * so a mis-transcription is caught and fixed here rather than silently
 * driving a wrong code edit. "Send" hands the (possibly edited) transcript
 * to the plan-only endpoint; nothing about the LLM runs until then.
 */
export function VoiceReviewSheet({
  mode = 'voice',
  recordingUri,
  transcript,
  onTranscriptChange,
  onReRecord,
  onCancel,
  onSend,
  testID = 'VoiceReviewSheet',
}: VoiceReviewSheetProps) {
  return (
    <Sheet
      open
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      snapPoints={[70, 50]}
      testID={testID}
    >
      <SheetHeader testID={`${testID}-Header`}>
        <XStack items="center" gap="$3">
          <Text flex={1} variant="strong" testID={`${testID}-Title`}>
            {mode === 'text'
              ? 'Describe the edit'
              : 'Review before sending'}
          </Text>
          <IconButton
            testID={`${testID}-Close`}
            icon={X}
            variant="ghost"
            aria-label="Cancel voice edit"
            onPress={onCancel}
          />
        </XStack>
      </SheetHeader>
      <YStack gap="$3" px="$4" pb="$4" testID={`${testID}-Content`}>
        {mode === 'voice' ? (
          <PlaybackRow
            recordingUri={recordingUri}
            testID={`${testID}-Playback`}
          />
        ) : null}

        <Text variant="muted" testID={`${testID}-TranscriptLabel`}>
          {mode === 'text' ? 'Instruction' : 'Transcript'}
        </Text>
        {transcript === null ? (
          <XStack
            items="center"
            gap="$2"
            py="$4"
            testID={`${testID}-Transcribing`}
          >
            <Spinner size="small" />
            <Text variant="muted">Transcribing…</Text>
          </XStack>
        ) : (
          <TextInput
            testID={`${testID}-TranscriptInput`}
            value={transcript}
            onChangeText={onTranscriptChange}
            multiline
            autoFocus={mode === 'text'}
            placeholder={
              mode === 'text'
                ? 'e.g. add a while loop that runs while left is less than right'
                : 'What you said…'
            }
            style={{
              minHeight: 88,
              borderRadius: 8,
              borderWidth: 1,
              borderColor: '#8884',
              paddingHorizontal: 10,
              paddingVertical: 8,
              fontSize: 15,
              textAlignVertical: 'top',
            }}
          />
        )}

        <XStack gap="$2.5" pt="$1">
          {mode === 'voice' ? (
            <Button
              testID={`${testID}-ReRecord`}
              variant="outline"
              flex={1}
              icon={RotateCcw}
              pressLog="off"
              onPress={onReRecord}
            >
              Re-record
            </Button>
          ) : null}
          <Button
            testID={`${testID}-Send`}
            flex={1}
            icon={Send}
            disabled={transcript === null || !transcript.trim()}
            pressLog="off"
            onPress={onSend}
          >
            Send
          </Button>
        </XStack>
      </YStack>
    </Sheet>
  );
}
