import { afterEach, describe, expect, it, vi } from 'vitest';

import { sendEditStats, sendVoiceTranscribe } from '../voiceEditTransport';

class RecordingFormData {
  entries: unknown[][] = [];
  append(...args: unknown[]) {
    this.entries.push(args);
  }
}

describe('sendVoiceTranscribe on a phone (Platform.OS is ios in the unit stub)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('Given a recording on a phone, When it is sent, Then the upload goes through XMLHttpRequest with the file as { uri, name, type }', async () => {
    // Given the platform's FormData, a fetch that must stay unused (reading the clip as a Blob makes expo/fetch warn), and an XMLHttpRequest that answers the upload
    const sent: RecordingFormData[] = [];
    class FakeXhr {
      status = 200;
      responseText = JSON.stringify({ transcript: 'hello' });
      timeout = 0;
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      ontimeout: (() => void) | null = null;
      open() {}
      send(body: RecordingFormData) {
        sent.push(body);
        queueMicrotask(() => this.onload?.());
      }
    }
    vi.stubGlobal('FormData', RecordingFormData);
    vi.stubGlobal('XMLHttpRequest', FakeXhr);
    const fetchMock = vi.fn(async () => new Response(new Blob([new Uint8Array(8)])));
    vi.stubGlobal('fetch', fetchMock);

    // When the recording is sent for transcription
    const result = await sendVoiceTranscribe({ audioUri: 'file:///cache/clip.m4a', mimeType: 'audio/m4a', filename: 'clip.m4a' });

    // Then the form went out through XHR, carrying the bare object, no Blob was read, and the answer came back
    expect(fetchMock).not.toHaveBeenCalled();
    expect(result.transcript).toBe('hello');
    expect(sent).toHaveLength(1);
    expect(sent[0]?.entries).toContainEqual([
      'audio',
      { uri: 'file:///cache/clip.m4a', name: 'clip.m4a', type: 'audio/m4a' },
      'clip.m4a',
    ]);
  });

  it('Given the upload fails at the network, When it is sent, Then a transport error with status 0 comes back', async () => {
    // Given an XMLHttpRequest that errors
    class FailingXhr {
      timeout = 0;
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      ontimeout: (() => void) | null = null;
      open() {}
      send() {
        queueMicrotask(() => this.onerror?.());
      }
    }
    vi.stubGlobal('FormData', RecordingFormData);
    vi.stubGlobal('XMLHttpRequest', FailingXhr);
    vi.stubGlobal('fetch', vi.fn(async () => new Response(new Blob([new Uint8Array(8)]))));

    // When the recording is sent
    const sending = sendVoiceTranscribe({ audioUri: 'file:///cache/clip.m4a', mimeType: 'audio/m4a', filename: 'clip.m4a' });

    // Then it rejects as a transport error, status 0
    await expect(sending).rejects.toMatchObject({ name: 'VoiceEditTransportError', status: 0 });
  });

  it('Given the first stats upload drops, When it is sent, Then it is retried once and succeeds', async () => {
    // Given a fetch whose first call fails with a network error and whose second answers
    vi.useFakeTimers();
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new TypeError('Network request failed'))
      .mockResolvedValueOnce(new Response(JSON.stringify({ ok: true })));
    vi.stubGlobal('fetch', fetchMock);

    // When the row is sent
    const done = sendEditStats({ editId: 'e1', outcome: 'applied' } as never);
    await vi.advanceTimersByTimeAsync(1_500);
    await done;
    vi.useRealTimers();

    // Then fetch was tried twice
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});
