import { BROWSER_CAPABILITIES } from '../../domain/settingsCapabilities';
import { OpenCapabilitySettingsScreen } from './OpenCapabilitySettingsScreen';

export function BrowserSettingsScreen() {
  return (
    <OpenCapabilitySettingsScreen
      title="Browser"
      note="This app shows no terminal output, so it has no terminal links to send to a browser."
      capabilities={BROWSER_CAPABILITIES}
    />
  );
}
