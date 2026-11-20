import type { EditorClient, Reply } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';

import type { ToEditorMessage } from '../../domain/ports';
import { toConfigPatch, type LegacyConfigPatch } from '../engine/configPatch';

/** The messages that reach the page through the typed editor API. */
export const FORWARDED_TYPES = ['undo', 'redo', 'updateConfig', 'focus', 'setSelection', 'insertAt', 'typeText'] as const;
/**
 * The messages that still travel as fire-and-forget scripts, and why: `init` creates the editor; `blur` waits for an iOS check of
 * the blur trick; `setText` needs the diff sync; the highlight and diff marks move with the runner jump and voice edit. The list
 * only shrinks.
 */
export const LEGACY_TYPES = [
  'init',
  'blur',
  'setText',
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

// the replies that say the typed path could not run at all: an idempotent message is sent again on the old script
const typedPathUnavailable = (reply: Reply<unknown>) =>
  !reply.ok && ['not-ready', 'unsupported', 'transport'].includes(reply.error.code);

// the replies that prove an edit was not applied (a transport failure does not: the edit may have landed)
const editNotApplied = (reply: Reply<unknown>) =>
  !reply.ok && ['not-ready', 'unsupported'].includes(reply.error.code);

const reportFailure = (type: string) => (reply: Reply<unknown>) => {
  if (!reply.ok)
    Logger.warn('editor.port.forward_failed', { type, code: reply.error.code });
};

/** The editor port's `post`: the forwarded messages become typed API calls, everything else takes the old script path. */
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
        // idempotent, so a reply that did not focus the editor (not ready yet, a lost channel) falls back to the old script
        void client.focus().then((reply) => {
          if (reply.ok) return;
          reportFailure('focus')(reply);
          legacyPost(msg);
        });
        return;
      case 'setSelection':
        // out-of-range or invalid is logged and nothing moves, where the old script threw inside the page
        void client.select(msg.start, msg.end).then((reply) => {
          if (reply.ok) {
            // the old script focused the editor with the selection
            void client.focus().then(reportFailure('setSelection'));
            return;
          }
          reportFailure('setSelection')(reply);
          if (typedPathUnavailable(reply)) legacyPost(msg);
        });
        return;
      case 'insertAt': {
        const text = msg.text.replace(/\r\n?/g, '\n');
        const offset = msg.offset ?? 0;
        // a caret past the end of the insert is not a point inside it: the old script places it
        if (offset > 0 || -offset > text.length) {
          legacyPost(msg);
          return;
        }
        // at the selection, replacing it, and the caret where the old script put it: after the text, `offset` back from its end
        // (a point's offset is not negative, so it is counted from the start)
        void client
          .transact([
            { op: 'replace', range: 'selection', text },
            { op: 'select', anchor: { step: 0, edge: 'start', offset: text.length + offset } },
          ])
          .then((reply) => {
            if (reply.ok) return;
            reportFailure('insertAt')(reply);
            if (editNotApplied(reply)) legacyPost(msg);
          });
        return;
      }
      case 'typeText':
        // a keystroke through the editor's input handlers; a text that is not one (too long), or a page that cannot type, takes the old script
        void client.transact([{ op: 'type', text: msg.text }]).then((reply) => {
          if (reply.ok) return;
          reportFailure('typeText')(reply);
          if (editNotApplied(reply) || (!reply.ok && reply.error.code === 'invalid')) legacyPost(msg);
        });
        return;
      default:
        legacyPost(msg);
    }
  };
}
