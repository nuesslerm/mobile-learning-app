import { createEditorClient, type ApplyValue, type Language, type Range, type Reply, type Step } from '../../src';
import { createFakeEditor } from '../../src/testing';
import { lezerSnapshot } from './lezerPlain';

/** The two parsers a node step is planned over: the fake editor's tiny ones and the installed grammars. */
export const PARSERS = [
  ['the fake parser', true],
  ['Lezer', lezerSnapshot],
] as const;
export type Parser = (typeof PARSERS)[number][1];

/** What a call of node steps would do, asked as a preview so nothing is applied. */
export async function previewOf(parser: Parser, language: Language, text: string, steps: readonly Step[]): Promise<Reply<ApplyValue>> {
  const client = createEditorClient(createFakeEditor({ text, tree: parser }).engine);
  await client.setConfig({ language });
  return client.transact(steps, { preview: true });
}

/** The text a preview would leave, or the refusal that stopped it. */
export function outcomeOf(reply: Reply<ApplyValue>): string | { readonly code: string; readonly reason?: string; readonly range?: Range; readonly path?: string } {
  if (reply.ok) return reply.value.preview?.text ?? '(no preview)';
  const { error } = reply;
  return {
    code: error.code,
    ...('reason' in error && error.reason ? { reason: error.reason } : {}),
    ...('range' in error ? { range: error.range } : {}),
    ...('path' in error ? { path: error.path } : {}),
  };
}

/** The range of `first` through the end of `last` (or of `first` alone) in `text`. */
export function between(text: string, first: string, last = first): Range {
  const from = text.indexOf(first);
  if (from < 0) throw new Error(`the text has no ${JSON.stringify(first)}`);
  const end = text.indexOf(last, from);
  if (end < 0) throw new Error(`the text has no ${JSON.stringify(last)} after ${JSON.stringify(first)}`);
  return { from, to: end + last.length };
}
