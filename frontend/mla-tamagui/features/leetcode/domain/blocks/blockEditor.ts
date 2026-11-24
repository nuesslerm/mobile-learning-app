import type { EditorClient } from '@lib/code-editor-api';

/** What the editor hands the block view: its typed client, and a promise that resolves once the writes the editor still has queued have landed. */
export type BlockEditor = {
  readonly client: EditorClient;
  settled(): Promise<void>;
};
