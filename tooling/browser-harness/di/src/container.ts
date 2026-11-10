import type { ShellState, ShellInstance } from '@cli/harness-contracts';
import { readParams } from '@cli/harness-data';
import {
  setColorScheme,
  setDevice,
  setTarget,
  toggleKeyboard,
  sendKey,
  drainKeyQueue,
  getStatus,
} from '@cli/harness-use-cases';

export function buildShellContainer(
  onStateChange?: (state: ShellState) => void
): { state: ShellState; shell: ShellInstance } {
  let state = readParams();

  function update(next: ShellState): void {
    state = next;
    onStateChange?.(state);
  }

  const shell: ShellInstance = {
    setDevice(preset, orientation) {
      const next = setDevice(state, preset, orientation);
      if (next.lastError !== null && next.lastError !== state.lastError)
        return false;
      update(next);
      return true;
    },
    setColorScheme(scheme) {
      const next = setColorScheme(state, scheme);
      if (next.lastError !== null && next.lastError !== state.lastError)
        return false;
      update(next);
      return true;
    },
    setTarget(url) {
      const next = setTarget(state, url);
      if (next.lastError !== null) return false;
      update(next);
      return true;
    },
    toggleKeyboard(visible) {
      update(toggleKeyboard(state, visible));
    },
    getStatus() {
      return getStatus(state);
    },
    sendKey(key) {
      update(sendKey(state, key));
    },
    drainKeyQueue() {
      const result = drainKeyQueue(state);
      update(result.state);
      return result.key;
    },
  };

  return { state, shell };
}
