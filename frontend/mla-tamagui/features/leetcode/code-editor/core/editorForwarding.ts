import type { EditorClient, Reply } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';

import type { ToEditorMessage } from '../../domain/ports';
import { toConfigPatch, type LegacyConfigPatch } from '../engine/configPatch';

/** The messages that reach the page through the typed editor API. */
export const FORWARDED_TYPES = ['undo', 'redo', 'updateConfig', 'focus', 'setSelection'] as const;
/**
 * The messages that still travel as fire-and-forget scripts, and why: `init` creates the editor; `blur` waits for an iOS check of
 * the blur trick; `insertAt`/`typeText`/`setText` need the typing step and the diff sync; the highlight and diff marks move with the runner jump and voice edit. The list only shrinks.
 */
export const LEGACY_TYPES = [
  'init',
  'blur',
  'insertAt',
  'typeText',
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
      default:
        legacyPost(msg);
    }
  };
}
