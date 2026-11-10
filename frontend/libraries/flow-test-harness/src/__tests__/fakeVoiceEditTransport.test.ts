import { describe, expect, it } from 'vitest';

import { createFakeVoiceEditTransport } from '../doubles/fakeVoiceEditTransport';

const alternatives = [
  {
    id: 'alt-1',
    label: 'Compare and move pointers',
    summary: 'Replaces pass with the mismatch check and the two pointer moves.',
    confidence: 0.95,
    commands: [
      {
        type: 'replaceLines',
        startLine: 6,
        endLine: 6,
        newText: 'if s[left] != s[right]:\n    return False',
      },
    ],
  },
  {
    id: 'alt-2',
    label: 'Use a set',
    summary: 'Builds a set of seen values and checks membership.',
    confidence: 0.6,
    commands: [],
  },
];

describe('createFakeVoiceEditTransport', () => {
  it('given scripted transcript and alternatives, when transcribe, plan and sendEditStats run, then they resolve the scripted values with wallMs 0 and calls record every request', async () => {
    const transport = createFakeVoiceEditTransport({
      transcript: 'replace pass with a mismatch check',
      alternatives,
    });

    await expect(
      transport.transcribe({
        audioUri: 'fake://clip.m4a',
        mimeType: 'audio/m4a',
        filename: 'clip.m4a',
      })
    ).resolves.toEqual({ transcript: 'replace pass with a mismatch check', wallMs: 0 });

    await expect(
      transport.plan({
        transcript: 'replace pass with a mismatch check',
        currentCode: 'pass',
        language: 'python',
      })
    ).resolves.toEqual({
      transcript: 'replace pass with a mismatch check',
      alternatives,
      wallMs: 0,
    });

    await expect(
      transport.sendEditStats({ editId: 'edit-1', outcome: 'picked' })
    ).resolves.toBeUndefined();

    expect(transport.calls).toEqual(['transcribe', 'plan', 'sendEditStats']);
  });

  it('given failWith, when transcribe and plan run, then both reject with the same error and sendEditStats still resolves', async () => {
    const error = new Error('voice edit transport down');
    const transport = createFakeVoiceEditTransport({ failWith: error });

    await expect(
      transport.transcribe({
        audioUri: 'fake://clip.m4a',
        mimeType: 'audio/m4a',
        filename: 'clip.m4a',
      })
    ).rejects.toBe(error);
    await expect(
      transport.plan({ transcript: 't', currentCode: 'c', language: 'python' })
    ).rejects.toBe(error);
    await expect(transport.sendEditStats({ editId: 'edit-2' })).resolves.toBeUndefined();

    expect(transport.calls).toEqual(['transcribe', 'plan', 'sendEditStats']);
  });
});