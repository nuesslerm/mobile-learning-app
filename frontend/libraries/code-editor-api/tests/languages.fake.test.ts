import { describe, expect, it } from 'vitest';

import { createEditorClient, unwrap } from '../src';
import { createFakeEditor } from '../src/testing';

describe('an editor that offers one language', () => {
  it('given a fake restricted to python, when hello and configure({language: javascript}) are sent, then only python is offered and the switch is unsupported with the text and revision unchanged', async () => {
    // Given a fake editor that offers only python
    const fake = createFakeEditor({ text: 'abc', languages: ['python'] });
    const client = createEditorClient(fake.engine);

    // When hello is read and another language is requested
    const offered = unwrap(await client.hello()).capabilities.languages;
    const reply = await client.setConfig({ language: 'javascript' });

    // Then the list is exactly python, the request is unsupported and names the language, and nothing moved
    expect(offered).toEqual(['python']);
    expect(reply).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'language' } });
    expect(fake.peek()).toMatchObject({ text: 'abc', revision: 0 });
    expect(unwrap(await client.setConfig({})).language).toBe('python');
  });
});
