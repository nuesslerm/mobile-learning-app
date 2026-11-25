import { describe, expect, it } from 'vitest';
import { chatTranscript } from '../chatTranscript';
import type { T3Snapshot } from '../t3Mobile';

describe('Orca transcript from T3', () => {
  it('Given a photo read between two passages, when history is opened then the tool and the viewed image appear between those passages', () => {
    const photo = {
      id: 'blue',
      type: 'image' as const,
      name: 'blue.png',
      mimeType: 'image/png',
    };
    const snapshot: T3Snapshot = {
      thread: { id: 't', projectId: 'p', title: 'Chat' },
      messages: [
        {
          id: 'u',
          role: 'user',
          runId: 'r',
          text: 'Inspect this',
          attachments: [photo],
        },
        { id: 'a', role: 'assistant', runId: 'r', text: 'Checking 👋Done' },
      ],
      turnItems: [
        {
          id: 'tool',
          type: 'command_execution',
          name: 'read',
          runId: 'r',
          input: { filePath: '/fixture/blue.png' },
        },
      ],
      toolTextOffsets: { tool: 'Checking 👋'.length },
    };
    const rows = chatTranscript(snapshot);
    expect(
      rows.map((r) =>
        r.type === 'message' ? r.message.text : r.tools.map((t) => t.name)
      )
    ).toEqual(['Inspect this', 'Checking 👋', 'Done']);
    expect(rows[0]).toMatchObject({ message: { attachments: [] } });
    expect(rows[1]).toMatchObject({ activity: {tools: [{name: 'read'}], images: [photo]} });
  });
  it('Given a provider turn still streaming, when a turn item precedes the final conversation message then its text appears only once', () => {
    const snapshot: T3Snapshot = {
      thread: { id: 't', projectId: 'p', title: 'Chat' },
      messages: [{ id: 'u', role: 'user', runId: 'r', text: 'Hello' }],
      turnItems: [
        {
          id: 'item',
          type: 'assistant_message',
          messageId: 'a',
          runId: 'r',
          text: 'Hello!',
        },
      ],
    };
    const texts = () =>
      chatTranscript(snapshot).map((r) =>
        r.type === 'message' ? r.message.text : 'tool'
      );
    expect(texts()).toEqual(['Hello', 'Hello!']);
    snapshot.messages.push({
      id: 'a',
      role: 'assistant',
      runId: 'r',
      text: 'Hello!',
    });
    expect(texts()).toEqual(['Hello', 'Hello!']);
  });
});
