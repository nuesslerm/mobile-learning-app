import { TERMINAL_CAPABILITIES } from '../../domain/settingsCapabilities';
import { OpenCapabilitySettingsScreen } from './OpenCapabilitySettingsScreen';

export function TerminalSettingsScreen() {
  return (
    <OpenCapabilitySettingsScreen
      title="Terminal"
      note="This app runs no terminal on the phone, so the original's terminal settings have nothing to change here."
      capabilities={TERMINAL_CAPABILITIES}
    />
  );
}
