export type RenderMode = 'device-frame' | 'plain';
export type KeyboardMode = 'mock' | 'none';
export type DeviceOrientation = 'portrait' | 'landscape';

/**
 * The appearance the app inside the frame is told the OS prefers.
 *
 * A first-class part of the device frame, alongside device and orientation:
 * headless Chromium answers LIGHT for `system` regardless of the host, so a
 * dark-theme app is rendered light unless this says otherwise, and a whole
 * defect class (a component that never sets its own colour) is invisible.
 * `prefers-color-scheme` cannot be set from page JS, so the daemon
 * re-emulates it (`wf browser start-device --color-scheme`) rather than the shell.
 */
export type ColorScheme = 'light' | 'dark' | 'system';

export const COLOR_SCHEMES: readonly ColorScheme[] = [
  'system',
  'light',
  'dark',
] as const;

export function isColorScheme(value: string): value is ColorScheme {
  return (COLOR_SCHEMES as readonly string[]).includes(value);
}

/**
 * `screen`: the display alone (default), the status bar and home indicator overlaid on the app as iOS does. `bezel`: the
 * hardware frame around it. `document`: for pages that are not the app under test (reports, galleries): the status bar and
 * home indicator take their own rows above and below the page instead of covering it.
 */
export type FrameStyle = 'screen' | 'bezel' | 'document';

export const FRAME_STYLES: readonly FrameStyle[] = ['screen', 'bezel', 'document'] as const;

export function isFrameStyle(value: string): value is FrameStyle {
  return (FRAME_STYLES as readonly string[]).includes(value);
}

/**
 * The exact `device` prop values `react-device-frameset` (v1.3.4) ships
 * bezel assets for. Kept as a literal union (not re-exported from the
 * library) so this package stays free of a runtime dependency on it —
 * a test cross-checks every
 * `frameset` entry below against the library's real `DeviceOptions` keys.
 */
export type FramesetDeviceName =
  | 'iPhone X'
  | 'iPhone 8'
  | 'iPhone 8 Plus'
  | 'iPhone 5s'
  | 'iPhone 5c'
  | 'iPhone 4s'
  | 'Galaxy Note 8'
  | 'Nexus 5'
  | 'Lumia 920'
  | 'Samsung Galaxy S5'
  | 'HTC One'
  | 'iPad Mini'
  | 'MacBook Pro';

/**
 * A device the library ships an accurate bezel asset for. `color` must be
 * one of that device's supported colors (unsupported by devices with no
 * `colors`, e.g. 'iPad Mini' / 'Galaxy Note 8' — omit it there).
 */
export type FramesetBezel = {
  kind: 'frameset';
  framesetDevice: FramesetDeviceName;
  color?: string;
};

/**
 * Escape hatch for devices the installed frameset library has no accurate
 * asset for (every modern iPhone, and any full-size/Pro iPad). Rendered by
 * the harness UI as a generated CSS + inline-SVG bezel — no binary asset to
 * source or vendor. `insetRect` is the bezel thickness on each side, in CSS
 * px at 1x zoom; the rendered frame box is `viewport + insetRect`.
 */
export type CustomBezelAsset = 'phone-dynamic-island' | 'tablet-plain';

export type CustomBezel = {
  kind: 'custom';
  asset: CustomBezelAsset;
  insetRect: SafeAreaInsets;
};

/**
 * The bare-viewport, no-bezel mode -- a plain browser window with no phone
 * or tablet frame drawn around it. Used by the `desktop` preset, which is
 * only ever selected alongside `renderMode: 'plain'` (the harness-shell UI
 * that renders `DeviceFrame`'s bezel switch is never loaded in that mode,
 * see `data/browser/session.ts`'s `renderMode === 'device-frame'` gate), so
 * this exists for type-safety/exhaustiveness at the bezel switch rather
 * than because any code path actually draws it.
 */
export type NoBezel = { kind: 'none' };

export type DeviceBezel = FramesetBezel | CustomBezel | NoBezel;

export type SafeAreaInsets = {
  top: number;
  right: number;
  bottom: number;
  left: number;
};

/**
 * How the platform reacts to its own on-screen keyboard. This is a real
 * behavioural split, not a cosmetic one, and getting it wrong hides exactly
 * the bugs a device frame exists to catch.
 *
 * - `overlay` (iOS/WebKit): the layout viewport does NOT shrink. The keyboard
 *   is an independent system window over the document; the OS grows the bottom
 *   safe-area inset and scrolls the visual viewport to keep the caret in view.
 *   Anything pinned to `bottom: 0` without safe-area awareness stays COVERED.
 *   Matches `interactive-widget=overlays-content`.
 * - `resize` (Android/Chromium, and `adjustResize`): the window manager shrinks
 *   the layout viewport by the keyboard's height. Flexible containers compress
 *   and bottom-anchored bars ride up flush against the keyboard on their own.
 *   Matches `interactive-widget=resizes-content`.
 *
 * The harness shrank the app iframe unconditionally before this existed, i.e.
 * it emulated Android on every device -- so an iOS layout that covers its own
 * toolbar looked fine in an iPhone frame.
 */
export type KeyboardLayoutStrategy = 'overlay' | 'resize';

export type DevicePlatform = 'ios' | 'android' | 'desktop';

export type DeviceSpec = {
  id: DevicePreset;
  label: string;
  platform: DevicePlatform;
  bezel: DeviceBezel;
  /** Portrait CSS-pixel viewport (logical points), matching the real device. */
  viewport: { width: number; height: number };
  dpr: number;
  /** Portrait safe-area insets (notch/Dynamic Island/home indicator/iPad). */
  safeArea: SafeAreaInsets;
  supportsLandscape: boolean;
  /** Approximate display corner radius in CSS px; visual only, never used for layout. */
  screenCornerRadius: number;
  /** How the app's WebView identifies itself on this device; absent keeps Chromium's own. */
  identity?: DeviceIdentity;
};

/**
 * What web code inside the native app reads about its host: the system
 * WebView's user agent, `navigator.platform` and, where the engine sends them,
 * client hints. CodeMirror, for one, picks its Android input path from the
 * user agent. `{chrome}` stands for the driving Chromium's major version.
 * WebKit sends no client hints, and `navigator.vendor` stays Chromium's, so
 * WebKit-only branches keyed on the vendor still take the Blink path.
 */
export type DeviceIdentity = {
  userAgent: string;
  /** `navigator.platform`. */
  platform: string;
  /** iOS and Android both report 5. */
  maxTouchPoints: number;
  userAgentMetadata?: {
    brands: { brand: string; version: string }[];
    platform: string;
    platformVersion: string;
    architecture: string;
    model: string;
    mobile: boolean;
  };
};

const IPHONE_WEBVIEW: DeviceIdentity = {
  userAgent:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148',
  platform: 'iPhone',
  maxTouchPoints: 5,
};

// iPadOS WebViews present as desktop Safari on a Mac with touch points.
const IPAD_WEBVIEW: DeviceIdentity = {
  userAgent:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)',
  platform: 'MacIntel',
  maxTouchPoints: 5,
};

const PIXEL_8_WEBVIEW: DeviceIdentity = {
  userAgent:
    'Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/AP2A.240805.005; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/{chrome}.0.0.0 Mobile Safari/537.36',
  platform: 'Linux armv81',
  maxTouchPoints: 5,
  userAgentMetadata: {
    brands: [
      { brand: 'Android WebView', version: '{chrome}' },
      { brand: 'Chromium', version: '{chrome}' },
      { brand: 'Not=A?Brand', version: '24' },
    ],
    platform: 'Android',
    platformVersion: '14.0.0',
    architecture: '',
    model: 'Pixel 8',
    mobile: true,
  },
};

// One entry per device — the single source of truth for the daemon (reads
// viewport/safeArea) and the harness UI (reads bezel). Adding a device is a
// one-entry change here; nothing else needs to know about it.
export const DEVICE_REGISTRY: readonly DeviceSpec[] = [
  {
    id: 'iphone-se',
    platform: 'ios',
    label: 'iPhone SE',
    // The 2022 iPhone SE reuses the iPhone 8 body verbatim.
    bezel: { kind: 'frameset', framesetDevice: 'iPhone 8', color: 'black' },
    viewport: { width: 375, height: 667 },
    dpr: 2,
    safeArea: { top: 20, right: 0, bottom: 0, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 0,
    identity: IPHONE_WEBVIEW,
  },
  {
    id: 'iphone-8',
    platform: 'ios',
    label: 'iPhone 8',
    bezel: { kind: 'frameset', framesetDevice: 'iPhone 8', color: 'black' },
    viewport: { width: 375, height: 667 },
    dpr: 2,
    safeArea: { top: 20, right: 0, bottom: 0, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 0,
    identity: IPHONE_WEBVIEW,
  },
  {
    id: 'iphone-15',
    platform: 'ios',
    label: 'iPhone 15',
    bezel: {
      kind: 'custom',
      asset: 'phone-dynamic-island',
      insetRect: { top: 60, right: 16, bottom: 70, left: 16 },
    },
    viewport: { width: 393, height: 852 },
    dpr: 3,
    safeArea: { top: 59, right: 0, bottom: 34, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 55,
    identity: IPHONE_WEBVIEW,
  },
  {
    id: 'iphone-15-pro',
    platform: 'ios',
    label: 'iPhone 15 Pro',
    bezel: {
      kind: 'custom',
      asset: 'phone-dynamic-island',
      insetRect: { top: 58, right: 14, bottom: 68, left: 14 },
    },
    viewport: { width: 393, height: 852 },
    dpr: 3,
    safeArea: { top: 59, right: 0, bottom: 34, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 55,
    identity: IPHONE_WEBVIEW,
  },
  {
    id: 'iphone-16-pro',
    platform: 'ios',
    label: 'iPhone 16 Pro',
    bezel: {
      kind: 'custom',
      asset: 'phone-dynamic-island',
      insetRect: { top: 58, right: 14, bottom: 68, left: 14 },
    },
    viewport: { width: 402, height: 874 },
    dpr: 3,
    safeArea: { top: 59, right: 0, bottom: 34, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 62,
    identity: IPHONE_WEBVIEW,
  },
  {
    id: 'ipad-mini',
    platform: 'ios',
    label: 'iPad Mini',
    bezel: { kind: 'frameset', framesetDevice: 'iPad Mini' },
    viewport: { width: 744, height: 1133 },
    dpr: 2,
    safeArea: { top: 24, right: 0, bottom: 20, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 22,
    identity: IPAD_WEBVIEW,
  },
  {
    id: 'ipad',
    platform: 'ios',
    label: 'iPad',
    // No plain full-size 'iPad' asset in the installed frameset version —
    // the custom bezel avoids stretching the Mini asset to a different
    // aspect ratio.
    bezel: {
      kind: 'custom',
      asset: 'tablet-plain',
      insetRect: { top: 40, right: 40, bottom: 40, left: 40 },
    },
    viewport: { width: 820, height: 1180 },
    dpr: 2,
    safeArea: { top: 24, right: 0, bottom: 20, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 18,
    identity: IPAD_WEBVIEW,
  },
  {
    id: 'ipad-pro-11',
    platform: 'ios',
    label: 'iPad Pro 11"',
    bezel: {
      kind: 'custom',
      asset: 'tablet-plain',
      insetRect: { top: 36, right: 36, bottom: 36, left: 36 },
    },
    viewport: { width: 834, height: 1194 },
    dpr: 2,
    safeArea: { top: 24, right: 0, bottom: 20, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 18,
    identity: IPAD_WEBVIEW,
  },
  {
    id: 'ipad-pro-13',
    platform: 'ios',
    label: 'iPad Pro 13"',
    bezel: {
      kind: 'custom',
      asset: 'tablet-plain',
      insetRect: { top: 36, right: 36, bottom: 36, left: 36 },
    },
    viewport: { width: 1024, height: 1366 },
    dpr: 2,
    safeArea: { top: 24, right: 0, bottom: 20, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 18,
    identity: IPAD_WEBVIEW,
  },
  {
    id: 'pixel-8',
    platform: 'android',
    label: 'Pixel 8',
    // The library ships no accurate Pixel frame; Galaxy Note 8 is the
    // closest same-era Android slab silhouette.
    bezel: { kind: 'frameset', framesetDevice: 'Galaxy Note 8' },
    viewport: { width: 412, height: 915 },
    dpr: 2.625,
    safeArea: { top: 24, right: 0, bottom: 16, left: 0 },
    supportsLandscape: true,
    screenCornerRadius: 28,
    identity: PIXEL_8_WEBVIEW,
  },
  {
    id: 'desktop',
    platform: 'desktop',
    label: 'Desktop',
    // Only ever paired with renderMode: 'plain' -- see NoBezel's doc comment.
    bezel: { kind: 'none' },
    viewport: { width: 1440, height: 900 },
    dpr: 1,
    safeArea: { top: 0, right: 0, bottom: 0, left: 0 },
    supportsLandscape: false,
    screenCornerRadius: 0,
  },
] as const;

export type DevicePreset =
  | 'iphone-se'
  | 'iphone-8'
  | 'iphone-15'
  | 'iphone-15-pro'
  | 'iphone-16-pro'
  | 'ipad-mini'
  | 'ipad'
  | 'ipad-pro-11'
  | 'ipad-pro-13'
  | 'pixel-8'
  | 'desktop';

export const DEVICE_PRESET_IDS = DEVICE_REGISTRY.map((d) => d.id) as [
  DevicePreset,
  ...DevicePreset[],
];

export const DEFAULT_DEVICE_PRESET: DevicePreset = 'iphone-15-pro';

export function isDevicePreset(value: string): value is DevicePreset {
  return (DEVICE_PRESET_IDS as readonly string[]).includes(value);
}

export function getDeviceSpecById(id: DevicePreset): DeviceSpec {
  const spec = DEVICE_REGISTRY.find((d) => d.id === id);
  if (!spec) throw new Error(`Unknown device preset: ${id}`);
  return spec;
}

export function resolveOrientedViewport(
  spec: DeviceSpec,
  orientation: DeviceOrientation
): { width: number; height: number } {
  if (orientation === 'portrait' || !spec.supportsLandscape)
    return spec.viewport;
  return { width: spec.viewport.height, height: spec.viewport.width };
}

/** Rotates insets 90° clockwise: portrait-top becomes landscape-right, etc. */
export function rotateInsetsClockwise(insets: SafeAreaInsets): SafeAreaInsets {
  const { top, right, bottom, left } = insets;
  return { top: left, right: top, bottom: right, left: bottom };
}

export function resolveOrientedSafeArea(
  spec: DeviceSpec,
  orientation: DeviceOrientation
): SafeAreaInsets {
  if (orientation === 'portrait' || !spec.supportsLandscape)
    return spec.safeArea;
  return rotateInsetsClockwise(spec.safeArea);
}

/**
 * The keyboard layout strategy this device's platform actually uses.
 *
 * Derived from `platform` rather than stored per device, because it is a
 * property of the OS, not of the hardware -- every iOS device overlays and
 * every Android device resizes. `desktop` never renders a mock keyboard (it is
 * only ever paired with `renderMode: 'plain'`); `resize` is returned for it so
 * the switch is total, and nothing reads it.
 */
/**
 * The mock keyboard's height in CSS px.
 *
 * Lives here rather than in the shell UI because the daemon publishes it to
 * the app as the overlay inset: on iOS the app iframe keeps its full height,
 * so nothing else tells the app how much of its own screen is covered.
 */
export const MOCK_KEYBOARD_HEIGHT = 270;

export function resolveKeyboardStrategy(spec: DeviceSpec): KeyboardLayoutStrategy {
  return spec.platform === 'ios' ? 'overlay' : 'resize';
}

/** Bezel border thickness, oriented — only meaningful for a `custom` bezel. */
export function resolveOrientedBezelInset(
  spec: DeviceSpec,
  orientation: DeviceOrientation
): SafeAreaInsets {
  if (spec.bezel.kind !== 'custom')
    return { top: 0, right: 0, bottom: 0, left: 0 };
  if (orientation === 'portrait' || !spec.supportsLandscape)
    return spec.bezel.insetRect;
  return rotateInsetsClockwise(spec.bezel.insetRect);
}

export type ShellStatus = {
  device: DevicePreset;
  orientation: DeviceOrientation;
  colorScheme: ColorScheme;
  targetUrl: string;
  keyboardVisible: boolean;
  lastError: string | null;
};

export type ShellBridge = {
  setDevice(preset: DevicePreset, orientation?: DeviceOrientation): boolean;
  setColorScheme(scheme: ColorScheme): boolean;
  setTarget(url: string): boolean;
  toggleKeyboard(visible?: boolean): void;
  getStatus(): ShellStatus;
  sendKey(key: string): void;
};

export type ShellInternal = {
  drainKeyQueue(): string | null;
};

export type ShellInstance = {} & ShellBridge & ShellInternal;

export type ShellState = {
  device: DevicePreset;
  orientation: DeviceOrientation;
  colorScheme: ColorScheme;
  frame: FrameStyle;
  /** False hides the caption under the screen, for clean screenshots. */
  chrome: boolean;
  targetUrl: string;
  keyboardVisible: boolean;
  keyQueue: string[];
  lastError: string | null;
};
