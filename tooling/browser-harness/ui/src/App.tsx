import {
  getDeviceSpecById,
  resolveKeyboardStrategy,
  type ShellState,
  type ShellInstance,
} from '@cli/harness-contracts';
import { registerShell } from '@cli/harness-data';
import { buildShellContainer } from '@cli/harness-di';
import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';

import { DeviceFrame } from './DeviceFrame.js';
import { KEYBOARD_HEIGHT, MockKeyboard, type ScreenAnchor } from './MockKeyboard.js';

import './shell.css';

const IFRAME_TIMEOUT_MS = 15_000;

function App() {
  const [state, setState] = useState<ShellState | null>(null);
  const [shell, setShell] = useState<ShellInstance | null>(null);
  const [iframeLoading, setIframeLoading] = useState(true);
  const [screenAnchor, setScreenAnchor] = useState<ScreenAnchor | null>(null);
  const iframeRef = useRef<HTMLIFrameElement | null>(null);

  useEffect(() => {
    const { state: initial, shell: runtime } = buildShellContainer(setState);
    registerShell(runtime);
    setShell(runtime);
    // `buildShellContainer` reads the initial device/target/orientation from
    // the URL synchronously but only reports *later* changes through the
    // `onStateChange` callback — the first render needs this explicit push,
    // or the shell never leaves the "Loading..." screen.
    setState(initial);
  }, []);

  // Track the app iframe's screen rect so the keyboard lands on the device's
  // screen. Where exactly depends on the platform's own strategy: Android has
  // already shrunk the iframe, so the keyboard fills the strip it vacated;
  // iOS has not, so the keyboard is drawn over the iframe's lower band.
  useEffect(() => {
    const measure = () => {
      const el = iframeRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const overlays =
        state?.device !== undefined &&
        resolveKeyboardStrategy(getDeviceSpecById(state.device)) === 'overlay';
      const top = overlays ? rect.bottom - KEYBOARD_HEIGHT : rect.bottom;
      const screen = el.closest<HTMLElement>('.mcp-screen, .mcp-custom-bezel-screen, .marvel-device .screen');
      const screenRect = screen?.getBoundingClientRect();
      const reachesBottom = screenRect !== undefined && Math.abs(screenRect.bottom - (top + KEYBOARD_HEIGHT)) < 2;
      const bottomRadius = reachesBottom && screen ? parseFloat(getComputedStyle(screen).borderBottomLeftRadius) || 0 : 0;
      setScreenAnchor({ left: rect.left, width: rect.width, top, bottomRadius });
    };
    measure();
    window.addEventListener('resize', measure);
    const interval = setInterval(measure, 300);
    return () => {
      window.removeEventListener('resize', measure);
      clearInterval(interval);
    };
  }, [
    state?.device,
    state?.orientation,
    state?.targetUrl,
    state?.keyboardVisible,
    iframeLoading,
  ]);

  useEffect(() => {
    if (!iframeLoading) return;
    const timer = setTimeout(() => {
      setState((prev) =>
        prev ? { ...prev, lastError: 'App not responding' } : prev
      );
    }, IFRAME_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [iframeLoading]);

  if (!state) {
    return (
      <div className="shell-loading">
        <p>Loading...</p>
      </div>
    );
  }

  const handleRetry = () => {
    setState((prev) => (prev ? { ...prev, lastError: null } : prev));
    setIframeLoading(true);
    if (shell && state.targetUrl) {
      shell.setTarget(state.targetUrl);
    }
  };

  return (
    <div>
      {state.lastError && (
        <div className="shell-error">
          <span>{state.lastError}</span>
          {state.lastError === 'App not responding' && (
            <button onClick={handleRetry}>Reload</button>
          )}
        </div>
      )}
      {state.targetUrl && (
        <DeviceFrame
          device={state.device}
          orientation={state.orientation}
          colorScheme={state.colorScheme}
          frame={state.frame}
          chrome={state.chrome}
          targetUrl={state.targetUrl}
          onLoad={() => setIframeLoading(false)}
          iframeRef={iframeRef}
          keyboardVisible={state.keyboardVisible}
        />
      )}
      <MockKeyboard visible={state.keyboardVisible} anchor={screenAnchor} />
    </div>
  );
}

const root = document.getElementById('root');
if (root) createRoot(root).render(<App />);
