import { createEditorClient, unwrap } from '@lib/code-editor-api';
import { waitFor } from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';

import { mountPage } from './support/mountEditorPage';

vi.setConfig({ testTimeout: 30_000, hookTimeout: 30_000 });

const ENTRIES = [{ label: 'quokka_total', type: 'variable', detail: 'int' }];

async function readyPage(language = 'python') {
  const page = await mountPage({ init: true, language });
  await waitFor(async () => {
    const reply = await page.engine.request('hello', undefined);
    if (!reply.ok || !reply.value.ready) throw new Error('the editor is not ready');
  });
  return { page, client: createEditorClient(page.engine) };
}

const popupLabels = (page: Awaited<ReturnType<typeof mountPage>>): string[] => {
  const doc = (page.frame() as unknown as { document?: Document } | null)?.document;
  return Array.from(doc?.querySelectorAll('.cm-tooltip-autocomplete li[role=option]') ?? [], (option) => option.textContent ?? '');
};

async function typed(client: ReturnType<typeof createEditorClient>, text: string) {
  for (const char of text) unwrap(await client.transact([{ op: 'type', text: char }]));
}

describe('the completion of the real CodeMirror page', () => {
  it('given completion switched on, when the config is read, then it carries the completion and the page announces the step and the event', async () => {
    const { client } = await readyPage();

    const config = unwrap(await client.setConfig({ completion: { enabled: true, entries: ENTRIES } }));

    expect(config.completion).toEqual({ enabled: true, entries: ENTRIES });
    const capabilities = unwrap(await client.hello()).capabilities;
    expect(capabilities.steps).toContain('complete');
    expect(capabilities.events).toContain('completion');
  });

  it('given completion on and then a load and a language switch, when a prefix is typed, then the list still opens', async () => {
    const { page, client } = await readyPage();
    unwrap(await client.setConfig({ completion: { enabled: true, entries: ENTRIES } }));
    unwrap(await client.load(''));
    unwrap(await client.setConfig({ language: 'javascript' }));
    unwrap(await client.load(''));

    await typed(client, 'qu');

    await waitFor(() => expect(popupLabels(page).some((label) => label.startsWith('quokka_total'))).toBe(true), { timeout: 5_000 });
    expect(unwrap(await client.setConfig({})).completion?.enabled).toBe(true);
  });

  it('given completion never configured, when a prefix is typed, then the page shows no list', async () => {
    const { page, client } = await readyPage();
    unwrap(await client.load(''));

    await typed(client, 'ret');
    await new Promise((resolve) => setTimeout(resolve, 400));

    expect(popupLabels(page)).toEqual([]);
    expect(unwrap(await client.setConfig({})).completion).toEqual({ enabled: false, entries: [] });
  });
});
