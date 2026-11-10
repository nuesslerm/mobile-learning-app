import {
  isColorScheme,
  isDevicePreset,
  type DeviceOrientation,
  type DevicePreset,
  type ColorScheme,
  type ShellState,
} from '@cli/harness-contracts';

export function setDevice(
  state: ShellState,
  preset: DevicePreset,
  orientation?: DeviceOrientation
): ShellState {
  if (!isDevicePreset(preset)) {
    return { ...state, lastError: `Unknown device: ${preset}` };
  }
  return {
    ...state,
    device: preset,
    orientation: orientation ?? state.orientation,
    lastError: null,
  };
}

export function setColorScheme(
  state: ShellState,
  scheme: ColorScheme
): ShellState {
  if (!isColorScheme(scheme)) {
    return { ...state, lastError: `Unknown color scheme: ${scheme}` };
  }
  return { ...state, colorScheme: scheme, lastError: null };
}

export function setTarget(state: ShellState, url: string): ShellState {
  try {
    new URL(url);
  } catch {
    return { ...state, lastError: `Invalid URL: ${url}` };
  }
  return { ...state, targetUrl: url, lastError: null };
}

export function toggleKeyboard(
  state: ShellState,
  visible?: boolean
): ShellState {
  return { ...state, keyboardVisible: visible ?? !state.keyboardVisible };
}

export function sendKey(state: ShellState, key: string): ShellState {
  return { ...state, keyQueue: [...state.keyQueue, key] };
}

export function drainKeyQueue(state: ShellState): {
  state: ShellState;
  key: string | null;
} {
  const [first, ...rest] = state.keyQueue;
  return { state: { ...state, keyQueue: rest }, key: first ?? null };
}

export function getStatus(state: ShellState) {
  return {
    device: state.device,
    orientation: state.orientation,
    colorScheme: state.colorScheme,
    targetUrl: state.targetUrl,
    keyboardVisible: state.keyboardVisible,
    lastError: state.lastError,
  };
}
