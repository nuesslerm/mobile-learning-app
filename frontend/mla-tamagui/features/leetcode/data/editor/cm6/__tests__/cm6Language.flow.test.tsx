import { createEditorClient, unwrap } from '@lib/code-editor-api';
import { waitFor } from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';

import { mountPage } from './support/mountEditorPage';

vi.setConfig({ testTimeout: 30_000, hookTimeout: 30_000 });

async function readyPage(language: string, contrastCanvas = false) {
  const page = await mountPage({ init: true, language, contrastCanvas });
  await waitFor(async () => {
    const reply = await page.engine.request('hello', undefined);
    if (!reply.ok || !reply.value.ready) throw new Error('the editor is not ready');
  });
  return { page, client: createEditorClient(page.engine) };
}

describe('the language of the real CodeMirror page', () => {
  it('given the page initialised with javascript, when the config is read, then it names javascript and the page offers all three languages', async () => {
    const { client } = await readyPage('javascript');

    expect(unwrap(await client.setConfig({})).language).toBe('javascript');
    expect(unwrap(await client.hello()).capabilities.languages).toEqual(['python', 'javascript', 'cpp']);
  });

  it('given a page on javascript, when the language is set to python and a document is loaded, then the config still names python', async () => {
    const { client } = await readyPage('javascript');

    expect(unwrap(await client.setConfig({ language: 'python' })).language).toBe('python');
    unwrap(await client.load('x = 1'));

    expect(unwrap(await client.setConfig({})).language).toBe('python');
  });

  it('given a switch of language, when text was typed before it, then the text, the revision and the undo history are unchanged by the switch', async () => {
    const { client } = await readyPage('python');
    unwrap(await client.load('abc'));
    const typed = await client.insertAt(3, 'd');
    const before = unwrap(await client.getText());

    unwrap(await client.setConfig({ language: 'cpp' }));

    const after = await client.getText();
    expect(unwrap(after)).toEqual(before);
    expect(after.revision).toBe(typed.revision);
    unwrap(await client.undo());
    expect(unwrap(await client.getText()).text).toBe('abc');
  });

  it('given JavaScript on the contrast canvas, when an initializer becomes an arrow function, then its binding picks up the function color', async () => {
    const { page, client } = await readyPage('javascript', true);
    const code = 'const count = 1, solve = function(input) {}, map = value';
    unwrap(await client.load(code));
    const names = () => Array.from(
      page.frame()?.document.querySelectorAll('.cm-function-name') ?? [],
      (binding) => binding.textContent,
    );
    expect(names()).toEqual(['solve']);

    unwrap(await client.insertAt(code.length, ' => value'));
    expect(names()).toEqual(['solve', 'map']);

    unwrap(await client.setConfig({ language: 'python' }));
    expect(names()).toEqual([]);
  });

  it('given Python on the contrast canvas, when annotations use a union and a generic, then only names in the annotations get the type color', async () => {
    const { page, client } = await readyPage('python', true);
    unwrap(await client.load('def solve(node: ListNode | None) -> Optional[ListNode]:\n    return node.next'));
    const names = Array.from(
      page.frame()?.document.querySelectorAll('.cm-type-name') ?? [],
      (reference) => reference.textContent,
    );
    expect(names).toEqual(['ListNode', 'Optional', 'ListNode']);
  });

  it('given an init with a language the page does not know, when the config is read, then the page started on python', async () => {
    const { client } = await readyPage('rust');

    expect(unwrap(await client.setConfig({})).language).toBe('python');
  });
});
