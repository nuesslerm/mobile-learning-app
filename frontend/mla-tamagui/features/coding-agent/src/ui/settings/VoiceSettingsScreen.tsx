import { VOICE_CAPABILITIES } from '../../domain/settingsCapabilities';
import { OpenCapabilitySettingsScreen } from './OpenCapabilitySettingsScreen';

export function VoiceSettingsScreen() {
  return (
    <OpenCapabilitySettingsScreen
      title="Voice"
      note="Dictation records on a paired desktop, which this app does not drive yet."
      capabilities={VOICE_CAPABILITIES}
    />
  );
}
