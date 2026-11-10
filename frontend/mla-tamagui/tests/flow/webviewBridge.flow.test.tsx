import { createRef } from 'react';
import { renderScreen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import WebWebView, {
  type WebViewHandle,
} from '../../lib/webview/ReactNativeWebView.web';

/**
 * The page a WebView hosts talks to the app through two primitives only: `window.ReactNativeWebView.postMessage(string)`
 * going out and scripts the app injects coming in. react-native-webview gives a phone exactly that; the app's own
 * iframe shim gives the browser the same. The same scripted exchange runs against both, so a message that survives
 * one transport and not the other fails here and not on a device.
 */
const PAGE_SCRIPT = `
  window.__echo = function (value) { window.ReactNativeWebView.postMessage(JSON.stringify({ echo: value })); };
  window.ReactNativeWebView.postMessage('ready');
`;

const SCRIPT: string[] = [
  'window.__echo(2);',
  'window.ReactNativeWebView.postMessage(String(42));',
  'throw new Error("a script that throws");',
  'window.__echo("after the throw");',
  'window.__echo(1); window.__echo(2); window.__echo(3);',
];

const EXPECTED = [
  'ready',
  '{"echo":2}',
  '42',
  '{"echo":"after the throw"}',
  '{"echo":1}',
  '{"echo":2}',
  '{"echo":3}',
];

type Transport = {
  received: string[];
  thrown: string[];
  inject: (script: string) => void;
};

/** A WebView as a phone has it: postMessage is synchronous and takes a string only, injected scripts run in the page's scope. */
function mountNative(): Transport {
  const received: string[] = [];
  const thrown: string[] = [];
  const page = {
    ReactNativeWebView: {
      postMessage(data: unknown) {
        if (typeof data !== 'string')
          throw new TypeError(
            'react-native-webview postMessage takes a string'
          );
        received.push(data);
      },
    },
  } as Record<string, unknown>;
  const run = (script: string) =>
    new Function('window', `with (window) { ${script} }`)(page);
  run(PAGE_SCRIPT);
  return {
    received,
    thrown,
    inject: (script) => {
      try {
        run(script);
      } catch (err) {
        // The native bridge swallows a throwing injected script; the page's error handler sees it.
        if (!(err instanceof Error)) throw err;
        thrown.push(err.message);
      }
    },
  };
}

async function mountWeb(): Promise<Transport> {
  const received: string[] = [];
  const ref = createRef<WebViewHandle>();
  const html = `<!doctype html><html><head></head><body><script>${PAGE_SCRIPT}</script></body></html>`;
  renderScreen(
    <WebWebView
      ref={ref}
      source={{ html }}
      onMessage={(event) => received.push(event.nativeEvent.data)}
    />
  );
  await waitFor(() => expect(received).toEqual(['ready']));
  const thrown: string[] = [];
  // the page's error handler takes the throw, as on a phone, so it is not an uncaught error in the frame
  document
    .querySelector('iframe')
    ?.contentWindow?.addEventListener('error', (event) => {
      event.preventDefault();
      thrown.push(event.message.replace(/^Uncaught (Error: )?/, ''));
    });
  return {
    received,
    thrown,
    inject: (script) => ref.current?.injectJavaScript(script),
  };
}

/** Messages heard in all after each script above, 'ready' included: the throwing script says nothing. */
const HEARD_AFTER = [2, 3, 3, 4, 7];

async function drive(transport: Transport) {
  for (const [index, script] of SCRIPT.entries()) {
    transport.inject(script);
    await waitFor(() =>
      expect(transport.received).toEqual(EXPECTED.slice(0, HEARD_AFTER[index]))
    );
  }
}

describe('the WebView bridge', () => {
  it('Given the scripted exchange, When a phone-shaped WebView runs it, Then the app hears exactly the expected messages', async () => {
    // Given a phone-shaped bridge: synchronous, string-only postMessage
    const transport = mountNative();

    // When the scripted exchange runs
    await drive(transport);

    // Then the app received the messages in order, and the page saw the one throw
    expect(transport.received).toEqual(EXPECTED);
    expect(transport.thrown).toEqual(['a script that throws']);
  });

  it('Given the same exchange, When the browser shim runs it, Then the app hears exactly the same messages', async () => {
    // Given the app's iframe shim hosting the same page
    const transport = await mountWeb();

    // When the same scripted exchange runs
    await drive(transport);

    // Then the browser render delivers what a phone does: the same strings in the same order, the same one throw
    expect(transport.received).toEqual(EXPECTED);
    expect(transport.thrown).toEqual(['a script that throws']);
  });
});
