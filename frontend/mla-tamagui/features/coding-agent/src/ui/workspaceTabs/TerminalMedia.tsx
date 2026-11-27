import { pickTerminalImage } from './pickTerminalImage';
import { useEffect, useRef, useState } from 'react';
import { Pressable, Text, View } from 'react-native';
import {
  AudioQuality,
  IOSOutputFormat,
  RecordingPresets,
  requestRecordingPermissionsAsync,
  setAudioModeAsync,
  useAudioRecorder,
} from 'expo-audio';
import { ImagePlus, Mic, Square } from 'lucide-react-native';
import { Logger } from '@lib/logging';
import type { Pairing } from '../../domain/mobile';
import type { WorkspaceTabsPort } from '../../domain/workspaceTabs/tabs';
import { TabControl, useTabColors } from './TabControls';

const options = {
  ...RecordingPresets.HIGH_QUALITY,
  extension: '.wav',
  numberOfChannels: 1,
  sampleRate: 16000,
  ios: {
    outputFormat: IOSOutputFormat.LINEARPCM,
    audioQuality: AudioQuality.HIGH,
    linearPCMBitDepth: 16,
    linearPCMIsBigEndian: false,
    linearPCMIsFloat: false,
  },
};
type Props = {
  pair: Pairing;
  checkout: string;
  api: WorkspaceTabsPort;
  editable: boolean;
  insert: (text: string) => void;
  notify: (message: string) => void;
};
export function TerminalMedia({
  pair,
  checkout,
  api,
  editable,
  insert,
  notify,
}: Props) {
  const c = useTabColors(),
    recorder = useAudioRecorder(options);
  const [recording, setRecording] = useState(false),
    [busy, setBusy] = useState(false);
  const live = useRef(true),
    working = useRef(false),
    held = useRef(false),
    holdStart = useRef<Promise<void> | null>(null);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
      if (recorder.isRecording)
        void recorder
          .stop()
          .then(() => setAudioModeAsync({ allowsRecording: false }))
          .catch((e) =>
            Logger.withError(e).warn('coding_agent.audio_stop_failed')
          );
    };
  }, [recorder]);
  const failure = (cause: unknown, fallback: string) => {
    if (live.current) notify(cause instanceof Error ? cause.message : fallback);
  };
  async function image() {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    try {
      const encoded = await pickTerminalImage();
      if (encoded === null) return;
      const jpeg = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
      const path = await api.terminalImage(pair, checkout, jpeg.buffer);
      if (live.current) insert("'" + path.replace(/'/g, "'\\''") + "' ");
    } catch (e) {
      Logger.withError(e).warn('coding_agent.terminal_image_failed');
      failure(e, 'Could not paste the image.');
    } finally {
      working.current = false;
      if (live.current) setBusy(false);
    }
  }
  async function toggle() {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    try {
      if (recorder.isRecording) {
        await recorder.stop();
        setRecording(false);
        await setAudioModeAsync({ allowsRecording: false });
        if (!recorder.uri) throw new Error('The recording could not be read.');
        const { File } = await import('expo-file-system');
        const file = new File(recorder.uri);
        try {
          const text = await api.transcribe(pair, await file.arrayBuffer());
          if (live.current && text) insert(text);
        } finally {
          if (file.exists) file.delete();
        }
      } else {
        if (!(await requestRecordingPermissionsAsync()).granted)
          throw new Error('Allow microphone access in Settings to dictate.');
        await setAudioModeAsync({
          allowsRecording: true,
          playsInSilentMode: true,
        });
        await recorder.prepareToRecordAsync();
        if (live.current) {
          recorder.record();
          setRecording(true);
        }
      }
    } catch (e) {
      Logger.withError(e).warn('coding_agent.terminal_dictation_failed');
      failure(e, 'Could not dictate into this terminal.');
      await setAudioModeAsync({ allowsRecording: false });
      if (live.current) setRecording(false);
    } finally {
      working.current = false;
      if (live.current) setBusy(false);
    }
  }
  return (
    <View style={{ flexDirection: 'row', gap: 5, alignItems: 'center' }}>
      <TabControl
        label="Attach image"
        disabled={!editable || busy || recording}
        onPress={() => void image()}
      >
        <ImagePlus size={18} color={c.muted} />
      </TabControl>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={recording ? 'Finish dictation' : 'Dictate'}
        disabled={!editable}
        onPressIn={() => {
          held.current = false;
        }}
        onLongPress={() => {
          if (working.current || recorder.isRecording) return;
          held.current = true;
          holdStart.current = toggle();
        }}
        onPressOut={() => {
          if (held.current) {
            void holdStart.current?.then(() => {
              if (live.current && recorder.isRecording) void toggle();
            });
          }
        }}
        onPress={() => {
          if (!held.current) void toggle();
          held.current = false;
        }}
        style={{
          width: 30,
          height: 30,
          alignItems: 'center',
          justifyContent: 'center',
          backgroundColor: c.surface,
          borderRadius: 15,
        }}
      >
        {busy ? (
          <Text style={{ color: c.muted }}>…</Text>
        ) : recording ? (
          <Square size={14} color={c.recording} />
        ) : (
          <Mic size={17} color={c.muted} />
        )}
      </Pressable>
    </View>
  );
}
