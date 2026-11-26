import {
  getDeviceSpecById,
  resolveKeyboardStrategy,
  resolveOrientedBezelInset,
  resolveOrientedSafeArea,
  resolveOrientedViewport,
  type ColorScheme,
  type DeviceOrientation,
  type DevicePreset,
  type DeviceSpec,
  type FrameStyle,
  type KeyboardLayoutStrategy,
} from '@cli/harness-contracts';
import type { Ref } from 'react';
import {
  DeviceFrameset,
  type DeviceFramesetProps,
} from 'react-device-frameset';
import 'react-device-frameset/styles/marvel-devices.min.css';

import { KEYBOARD_HEIGHT } from './MockKeyboard.js';

type Props = {
  device: DevicePreset;
  orientation: DeviceOrientation;
  colorScheme: ColorScheme;
  frame: FrameStyle;
  chrome: boolean;
  targetUrl: string;
  onLoad: () => void;
  iframeRef?: Ref<HTMLIFrameElement>;
  keyboardVisible?: boolean;
};

type IframeProps = Pick<
  Props,
  'iframeRef' | 'targetUrl' | 'onLoad' | 'keyboardVisible'
> & { keyboardStrategy: KeyboardLayoutStrategy };

function AppIframe({
  iframeRef,
  targetUrl,
  onLoad,
  keyboardVisible,
  keyboardStrategy,
}: IframeProps) {
  // Android RESIZES: the window manager takes the keyboard's height off the
  // layout viewport, so the iframe is genuinely shorter and the app reflows
  // upward on its own. iOS OVERLAYS: the layout viewport keeps its full
  // height and the keyboard is drawn on top, so anything pinned to the bottom
  // without safe-area awareness stays covered -- which is the behaviour worth
  // reproducing, since it is the one that ships bugs.
  const shrink = keyboardVisible && keyboardStrategy === 'resize';
  return (
    <iframe
      ref={iframeRef}
      title="App Frame"
      name="device-frame-app"
      // microphone/camera are delegated so a voice or capture flow can run
      // inside the frame at all: Permissions Policy denies getUserMedia in a
      // cross-origin iframe that was not granted them, and the failure surfaces
      // as a bare "Permission denied" that looks like a browser-level refusal
      // rather than a missing attribute here.
      allow="clipboard-read; clipboard-write; microphone; camera; autoplay"
      src={targetUrl}
      onLoad={onLoad}
      // `display:block` avoids inline-gap; height animates so the resize
      // reflow is visible rather than instantaneous.
      style={{
        display: 'block',
        border: 0,
        width: '100%',
        height: shrink ? `calc(100% - ${KEYBOARD_HEIGHT}px)` : '100%',
        transition: 'height 0.18s ease-out',
      }}
    />
  );
}

function hasDynamicIsland(spec: DeviceSpec): boolean {
  return spec.bezel.kind === 'custom' && spec.bezel.asset === 'phone-dynamic-island';
}

function StatusBarGlyphs({ platform }: { platform: DeviceSpec['platform'] }) {
  return (
    <span className="mcp-statusbar-glyphs">
      {platform === 'ios' && (
        <svg width="18" height="12" viewBox="0 0 18 12" aria-hidden="true">
          <rect x="0" y="8" width="3" height="4" rx="1" fill="currentColor" />
          <rect x="5" y="5.5" width="3" height="6.5" rx="1" fill="currentColor" />
          <rect x="10" y="3" width="3" height="9" rx="1" fill="currentColor" />
          <rect x="15" y="0" width="3" height="12" rx="1" fill="currentColor" />
        </svg>
      )}
      <svg
        width="16"
        height="12"
        viewBox="0 0 16 12"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        aria-hidden="true"
      >
        <path d="M1.5 4.6a9.5 9.5 0 0 1 13 0" />
        <path d="M4 7.2a5.8 5.8 0 0 1 8 0" />
        <circle cx="8" cy="10.2" r="1.3" fill="currentColor" stroke="none" />
      </svg>
      <svg width="27" height="13" viewBox="0 0 27 13" aria-hidden="true">
        <rect
          x="0.5"
          y="0.5"
          width="23"
          height="12"
          rx="3.5"
          fill="none"
          stroke="currentColor"
          opacity="0.4"
        />
        <rect x="2" y="2" width="20" height="9" rx="2" fill="currentColor" />
        <rect x="24.5" y="4.5" width="1.5" height="4" rx="0.75" fill="currentColor" opacity="0.4" />
      </svg>
    </span>
  );
}

// System bars are drawn inside the safe areas so content under them shows up
// as the collision it would be on a phone.
function ScreenFrame({
  spec,
  orientation,
  iframeProps,
  document = false,
}: {
  spec: DeviceSpec;
  orientation: DeviceOrientation;
  iframeProps: IframeProps;
  /** The chrome takes its own rows instead of overlaying the page (reports, galleries). */
  document?: boolean;
}) {
  const viewport = resolveOrientedViewport(spec, orientation);
  const safeArea = resolveOrientedSafeArea(spec, orientation);
  return (
    <div
      className={document ? 'mcp-screen mcp-screen--document' : 'mcp-screen'}
      data-mcp-frame={document ? 'document' : 'screen'}
      data-mcp-platform={spec.platform}
      style={{
        width: viewport.width,
        height: viewport.height,
        borderRadius: spec.screenCornerRadius,
      }}
    >
      <AppIframe {...iframeProps} />
      {safeArea.top > 0 && (
        <div
          className="mcp-statusbar"
          data-mcp-platform={spec.platform}
          style={{ height: safeArea.top }}
          aria-hidden="true"
        >
          <span className="mcp-statusbar-time">9:41</span>
          {hasDynamicIsland(spec) && <span className="mcp-statusbar-island" />}
          <StatusBarGlyphs platform={spec.platform} />
        </div>
      )}
      {safeArea.bottom > 0 && (
        <div
          className="mcp-home-indicator"
          data-mcp-platform={spec.platform}
          style={{ height: safeArea.bottom }}
          aria-hidden="true"
        >
          <span />
        </div>
      )}
    </div>
  );
}

/**
 * Escape hatch for devices `react-device-frameset` has no accurate asset
 * for: a CSS + inline-SVG bezel generated from the registry's `insetRect`,
 * sized to `viewport + insetRect` — no binary asset to source or vendor.
 */
function CustomBezel({
  asset,
  viewport,
  insetRect,
  iframeProps,
}: {
  asset: 'phone-dynamic-island' | 'tablet-plain';
  viewport: { width: number; height: number };
  insetRect: { top: number; right: number; bottom: number; left: number };
  iframeProps: IframeProps;
}) {
  const isPhone = asset === 'phone-dynamic-island';
  const frameWidth = viewport.width + insetRect.left + insetRect.right;
  const frameHeight = viewport.height + insetRect.top + insetRect.bottom;
  const cornerRadius = isPhone ? 52 : 28;

  return (
    <div
      className="mcp-custom-bezel"
      data-mcp-bezel-asset={asset}
      style={{
        width: frameWidth,
        height: frameHeight,
        borderRadius: cornerRadius,
        paddingTop: insetRect.top,
        paddingRight: insetRect.right,
        paddingBottom: insetRect.bottom,
        paddingLeft: insetRect.left,
      }}
    >
      {isPhone && (
        <svg
          className="mcp-custom-bezel-island"
          width={viewport.width * 0.32}
          height={insetRect.top * 0.42}
          viewBox="0 0 100 40"
          aria-hidden="true"
        >
          <rect x="0" y="0" width="100" height="40" rx="20" fill="#000" />
        </svg>
      )}
      <div className="mcp-custom-bezel-screen">
        <AppIframe {...iframeProps} />
      </div>
      <div className="mcp-custom-bezel-home-indicator" aria-hidden="true" />
    </div>
  );
}

function Bezel({
  spec,
  orientation,
  iframeProps,
}: {
  spec: DeviceSpec;
  orientation: DeviceOrientation;
  iframeProps: IframeProps;
}) {
  const viewport = resolveOrientedViewport(spec, orientation);
  if (spec.bezel.kind === 'frameset') {
    return (
      <DeviceFrameset
        {...({
          device: spec.bezel.framesetDevice,
          color: spec.bezel.color,
          width: viewport.width,
          height: viewport.height,
          landscape: orientation === 'landscape' ? true : undefined,
          zoom: 1,
        } as DeviceFramesetProps)}
      >
        <AppIframe {...iframeProps} />
      </DeviceFrameset>
    );
  }
  if (spec.bezel.kind === 'custom') {
    return (
      <CustomBezel
        asset={spec.bezel.asset}
        viewport={viewport}
        insetRect={resolveOrientedBezelInset(spec, orientation)}
        iframeProps={iframeProps}
      />
    );
  }
  // 'none' -- only the desktop preset, which plain render mode never frames.
  return <AppIframe {...iframeProps} />;
}

export function DeviceFrame({
  device,
  orientation,
  colorScheme,
  frame,
  chrome,
  targetUrl,
  onLoad,
  iframeRef,
  keyboardVisible,
}: Props) {
  const spec = getDeviceSpecById(device);
  const iframeProps: IframeProps = {
    iframeRef,
    targetUrl,
    onLoad,
    keyboardVisible,
    keyboardStrategy: resolveKeyboardStrategy(spec),
  };

  return (
    <div className="mcp-device-stage" data-mcp-frame-style={frame}>
      <div className="mcp-device-frame-wrapper">
        {frame === 'screen' || frame === 'document' ? (
          <ScreenFrame spec={spec} orientation={orientation} iframeProps={iframeProps} document={frame === 'document'} />
        ) : (
          <Bezel spec={spec} orientation={orientation} iframeProps={iframeProps} />
        )}
      </div>
      {chrome && (
        <div className="mcp-caption" data-testid="mcp-caption">
          {spec.label} · {orientation} · {colorScheme}
        </div>
      )}
    </div>
  );
}
