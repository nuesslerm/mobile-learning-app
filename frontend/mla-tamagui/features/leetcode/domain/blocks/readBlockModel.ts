import type { EditorClient, EditorErrorCode, Language, Reply, TreeValue } from '@lib/code-editor-api';

import { buildBlockModel, type BlockModel } from './blockModel';

/** How many times a tree is asked for while its parse is not complete: each ask lets the page parse a little more. */
export const MAX_TREE_ASKS = 12;

export type BlockRead =
  | { readonly kind: 'ready'; readonly model: BlockModel }
  | { readonly kind: 'not-ready' }
  /** the editor cannot give a tree or cannot apply the node steps the view makes */
  | { readonly kind: 'unavailable' }
  | { readonly kind: 'unsupported'; readonly language: Language }
  /** the text kept changing between the two reads */
  | { readonly kind: 'stale' }
  | { readonly kind: 'failed'; readonly code: EditorErrorCode };

async function askTree(client: EditorClient): Promise<Reply<TreeValue>> {
  let reply = await client.tree({ detail: 'outline' });
  for (let asks = 1; asks < MAX_TREE_ASKS && reply.ok && !reply.value.complete; asks += 1) reply = await client.tree({ detail: 'outline' });
  return reply;
}

/**
 * The model of what the editor holds: the text and the tree of one revision. A keystroke between the two reads is read again once, then `stale`; a tree whose parse is not complete is asked for again, up to
 * MAX_TREE_ASKS times, and then drawn as far as it reached. The editor must announce the tree and both node steps, and hold Python.
 */
export async function readBlockModel(client: EditorClient): Promise<BlockRead> {
  const hello = await client.hello();
  if (!hello.ok) return { kind: 'failed', code: hello.error.code };
  const { capabilities } = hello.value;
  if (!hello.value.ready) return { kind: 'not-ready' };
  if (!capabilities.requests.includes('tree') || !capabilities.steps.includes('moveNode') || !capabilities.steps.includes('insertSnippet')) return { kind: 'unavailable' };

  for (let attempt = 0; attempt < 2; attempt += 1) {
    const read = await client.getText();
    if (!read.ok) return { kind: 'failed', code: read.error.code };
    const tree = await askTree(client);
    if (!tree.ok) return tree.error.code === 'unsupported' ? { kind: 'unavailable' } : { kind: 'failed', code: tree.error.code };
    if (tree.value.language !== 'python') return { kind: 'unsupported', language: tree.value.language };
    if (read.revision === tree.value.revision) return { kind: 'ready', model: buildBlockModel({ text: read.value.text, tree: tree.value }) };
  }
  return { kind: 'stale' };
}
