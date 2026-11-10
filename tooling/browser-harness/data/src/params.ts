import {
  DEFAULT_DEVICE_PRESET,
  isColorScheme,
  isDevicePreset,
  isFrameStyle,
  type ColorScheme,
  type DeviceOrientation,
  type FrameStyle,
  type ShellState,
} from '@cli/harness-contracts';

function resolveDevice(raw: string | null): ShellState['device'] {
  if (raw && isDevicePreset(raw)) return raw;
  return DEFAULT_DEVICE_PRESET;
}

function resolveOrientation(raw: string | null): DeviceOrientation {
  return raw === 'landscape' ? 'landscape' : 'portrait';
}

function resolveColorScheme(raw: string | null): ColorScheme {
  return raw && isColorScheme(raw) ? raw : 'system';
}

function resolveFrame(raw: string | null): FrameStyle {
  return raw && isFrameStyle(raw) ? raw : 'screen';
}

export function readParams(): ShellState {
  const params = new URLSearchParams(window.location.search);
  return {
    device: resolveDevice(params.get('device')),
    orientation: resolveOrientation(params.get('orientation')),
    colorScheme: resolveColorScheme(params.get('colorScheme')),
    frame: resolveFrame(params.get('frame')),
    chrome: params.get('chrome') !== '0',
    targetUrl: params.get('target') ?? '',
    keyboardVisible: params.get('keyboard') === '1',
    keyQueue: [],
    lastError: null,
  };
}
