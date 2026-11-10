'use strict';
// Expo audio stub for flow tests. expo-audio cannot load under jsdom and is
// never exercised here -- the AudioRecordingPort is injected via the harness
// double -- so the exports just resolve the module graph's static imports.
// Shape mirrors the per-package vi.mock the flow tests previously duplicated.
module.exports = {
  __esModule: true,
  RecordingPresets: { HIGH_QUALITY: {} },
  requestRecordingPermissionsAsync: async () => ({ granted: true }),
  setAudioModeAsync: async () => undefined,
  useAudioRecorder: () => ({}),
  useAudioRecorderState: () => ({ metering: undefined }),
  useAudioPlayer: () => ({ play: () => {}, pause: () => {}, seekTo: () => {} }),
  useAudioPlayerStatus: () => ({ playing: false, currentTime: 0, duration: 0 }),
};