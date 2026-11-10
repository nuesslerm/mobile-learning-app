import { Logger } from '@lib/logging';

import { useKeyboardStore } from '../keyboardStore';

declare global {
  interface Window {
    /**
     * Set by the multi-browser harness (`wf browser`) when its on-screen
     * keyboard is raised. A real soft keyboard cannot exist in a headless
     * browser, and focus inside the sandboxed editor iframe is invisible here
     * (cross-origin), so the harness signals "keyboard is up" this way.
     */
    __MCP_KEYBOARD_VISIBLE__?: boolean;
    /**
     * How the emulated platform reacts to its keyboard. `resize` (Android)
     * shrinks the app's viewport, so visualViewport already reports the
     * overlap. `overlay` (iOS) does not, so the harness reports the height.
     */
    __MCP_KEYBOARD_MODE__?: 'overlay' | 'resize';
    /** Covered height in CSS px. Non-zero only in `overlay` mode. */
    __MCP_KEYBOARD_HEIGHT__?: number;
  }
}

const MCP_KEYBOARD_EVENT = 'mcp:keyboardframe';
const WEB_KEYBOARD_EASING = 'visualViewport';
const WEB_KEYBOARD_DURATION = 150;

function isIframe(element: Element): boolean {
  return element.tagName.toLowerCase() === 'iframe';
}

function hasEditableFocus(): boolean {
  if (typeof document === 'undefined') return false;
  const element = document.activeElement;
  if (!element) return false;

  if (isIframe(element)) {
    try {
      // eslint-disable-next-line no-restricted-syntax -- narrowing a DOM node; a guard would not add runtime safety
      const doc = (element as HTMLIFrameElement).contentDocument;
      const active = doc?.activeElement;
      if (!active) return false;
      const tag = active.tagName.toLowerCase();
      if (tag === 'input' || tag === 'textarea') return true;
      if (active.getAttribute('contenteditable') === 'true') return true;
      return active.getAttribute('role') === 'textbox';
    } catch (err) {
      Logger.withError(err).warn('keyboard.source.crossOriginIframe');
      return false;
    }
  }

  const tag = element.tagName.toLowerCase();
  if (tag === 'input' || tag === 'textarea') return true;
  return element.getAttribute('contenteditable') === 'true';
}

/**
 * Web keyboard source: visual-viewport geometry (and the harness's flags) into
 * the SAME store the native source writes to.
 *
 * This used to be a whole separate `useKeyboardFrame.web.ts` that read the
 * viewport directly and never touched the store -- which meant the web build
 * needed no wiring while native needed an observer component mounted. Forget
 * that component and web still worked perfectly while native showed no
 * keyboard toolbar at all, which is exactly what happened. Sharing the store
 * makes the wiring requirement identical on both platforms, so a web check can
 * actually speak to native.
 */
export function startKeyboardSource(): () => void {
  if (typeof window === 'undefined') return () => {};

  const { setKeyboardShown, setKeyboardHidden } = useKeyboardStore.getState();
  const viewport = window.visualViewport;
  let frameHandle: number | null = null;

  const publish = () => {
    const layoutHeight = window.innerHeight;
    const visualBottom = viewport ? viewport.height + viewport.offsetTop : layoutHeight;
    const overlap = Math.max(0, layoutHeight - visualBottom);
    const focused = hasEditableFocus();
    const harnessUp = window.__MCP_KEYBOARD_VISIBLE__ === true;
    // Under `resize` the harness already shrank the viewport, so `overlap`
    // carries the geometry and adding this would double-shift the layout.
    // Under `overlay` -- iOS's real behaviour -- the viewport keeps its full
    // height, so this is the only signal that the lower band is covered.
    const harnessInset =
      harnessUp && window.__MCP_KEYBOARD_MODE__ === 'overlay'
        ? (window.__MCP_KEYBOARD_HEIGHT__ ?? 0)
        : 0;

    const visible = focused || overlap > 0 || harnessUp;
    const height = Math.max(focused ? overlap : 0, harnessInset);
    const transition = { duration: WEB_KEYBOARD_DURATION, easing: WEB_KEYBOARD_EASING };

    if (visible) setKeyboardShown(height, transition);
    else setKeyboardHidden(transition);
  };

  const schedule = () => {
    if (frameHandle != null) return;
    frameHandle = window.requestAnimationFrame(() => {
      frameHandle = null;
      publish();
    });
  };

  viewport?.addEventListener('resize', schedule);
  viewport?.addEventListener('scroll', schedule);
  window.addEventListener('resize', schedule);
  window.addEventListener('scroll', schedule, true);
  window.addEventListener('orientationchange', schedule);
  window.addEventListener('focus', schedule);
  window.addEventListener('blur', schedule);
  document.addEventListener('focusin', schedule);
  document.addEventListener('focusout', schedule);
  window.addEventListener(MCP_KEYBOARD_EVENT, schedule);
  schedule();
  Logger.info('keyboard.source.started', { platform: 'web' });

  return () => {
    viewport?.removeEventListener('resize', schedule);
    viewport?.removeEventListener('scroll', schedule);
    window.removeEventListener('resize', schedule);
    window.removeEventListener('scroll', schedule, true);
    window.removeEventListener('orientationchange', schedule);
    window.removeEventListener('focus', schedule);
    window.removeEventListener('blur', schedule);
    document.removeEventListener('focusin', schedule);
    document.removeEventListener('focusout', schedule);
    window.removeEventListener(MCP_KEYBOARD_EVENT, schedule);
    if (frameHandle != null) window.cancelAnimationFrame(frameHandle);
  };
}
