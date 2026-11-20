import type { EditorClient, Reply } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';

import type { ToEditorMessage } from '../../domain/ports';
import { toConfigPatch, type LegacyConfigPatch } from '../engine/configPatch';

/** The messages that reach the page through the typed editor API. */
export const FORWARDED_TYPES = ['undo', 'redo', 'updateConfig', 'focus', 'setSelection', 'insertAt', 'typeText'] as const;
/**
 * The messages that still travel as fire-and-forget scripts, and why: `init` creates the editor; `blur` waits for an iOS check of
 * the blur trick; the highlight and diff marks move with the runner jump and voice edit. The list only shrinks.
 */
export const LEGACY_TYPES = [
  'init',
  'blur',
  'highlightRange',
  'clearHighlights',
  'showDiff',
  'hideDiff',
] as const;

// a message type that is on neither list fails the build here
type Unlisted = Exclude<
  ToEditorMessage['type'],
  (typeof FORWARDED_TYPES)[number] | (typeof LEGACY_TYPES)[number]
>;
export const everyMessageIsListed: [Unlisted] extends [never] ? true : never =
  true;

const reportFailure = (type: string) => (reply: Reply<unknown>) => {
  if (!reply.ok)
    Logger.warn('editor.port.forward_failed', { type, code: reply.error.code });
};

/**
 * The editor port's `post`: every message that acts on the document or the view is a typed API call, and a call the editor
 * refuses (not ready yet, a position outside the text, a lost channel) is logged and does nothing. The few messages that are not in
 * the API (`init`, `blur`, the marks, and an `updateConfig` the API cannot express) take the old script path.
 */
export function createForwardingPost(
  client: EditorClient,
  legacyPost: (msg: ToEditorMessage) => void
): (msg: ToEditorMessage) => void {
  return (msg) => {
    switch (msg.type) {
      case 'undo':
        void client.undo().then(reportFailure('undo'));
        return;
      case 'redo':
        void client.redo().then(reportFailure('redo'));
        return;
      case 'updateConfig': {
        const patch = toConfigPatch(msg.config as LegacyConfigPatch);
        if (!patch) {
          legacyPost(msg);
          return;
        }
        void client.setConfig(patch).then(reportFailure('updateConfig'));
        return;
      }
      case 'focus':
        void client.focus().then(reportFailure('focus'));
        return;
      case 'setSelection':
        void client.select(msg.start, msg.end).then((reply) => {
          reportFailure('setSelection')(reply);
          // a selection from the toolbar brings the editor into focus, as a tap would
          if (reply.ok) void client.focus().then(reportFailure('setSelection'));
        });
        return;
      case 'insertAt': {
        const text = msg.text.replace(/\r\n?/g, '\n');
        // after the text, `offset` back from its end (a point's offset is not negative, so it is counted from the start); a caret
        // past the end of the insert, or before its start, is held at the nearest end of it
        const offset = Math.max(-text.length, Math.min(0, msg.offset ?? 0));
        void client
          .transact([
            { op: 'replace', range: 'selection', text },
            { op: 'select', anchor: { step: 0, edge: 'start', offset: text.length + offset } },
          ])
          .then(reportFailure('insertAt'));
        return;
      }
      case 'typeText':
        // one keystroke through the editor's input handlers (a text longer than that is invalid and logged)
        void client.transact([{ op: 'type', text: msg.text }]).then(reportFailure('typeText'));
        return;
      default:
        legacyPost(msg);
    }
  };
}
