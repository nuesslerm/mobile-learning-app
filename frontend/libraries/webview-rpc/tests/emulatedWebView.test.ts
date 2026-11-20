import { describe, expect, it } from 'vitest';

import { deliverScript } from '../src';
import { createEmulatedWebView } from '../src/testing';
import { rpcContract } from './contract';

rpcContract('the host and page transports over an emulated WebView', createEmulatedWebView);

describe('the host transport', () => {
  it('given a string with quotes and line breaks, when it is delivered, then the injected script carries it as one JavaScript string', () => {
    // Given a payload that would break a naive script
    const raw = JSON.stringify({ text: 'say "hi"\nand \\ go', tail: ' ' });

    // When the script is built and evaluated against a window that records the event
    const seen: unknown[] = [];
    const win = { dispatchEvent: (event: { data: unknown }) => seen.push(event.data) };
    class FakeMessageEvent {
      constructor(
        readonly type: string,
        init: { data: unknown }
      ) {
        this.data = init.data;
      }
      readonly data: unknown;
    }
    new Function('window', 'MessageEvent', deliverScript(raw))(win, FakeMessageEvent);

    // Then the page saw the payload exactly as it was sent
    expect(seen).toEqual([raw]);
  });
});
