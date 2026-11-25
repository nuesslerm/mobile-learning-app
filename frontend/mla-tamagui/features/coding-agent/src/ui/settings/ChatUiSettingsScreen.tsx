import { CHAT_UI_CAPABILITIES } from '../../domain/settingsCapabilities';
import { OpenCapabilitySettingsScreen } from './OpenCapabilitySettingsScreen';

export function ChatUiSettingsScreen() {
  return (
    <OpenCapabilitySettingsScreen
      title="Chat UI"
      note="Sessions open in the chat view on this device. The original also offers the raw terminal view."
      capabilities={CHAT_UI_CAPABILITIES}
    />
  );
}
