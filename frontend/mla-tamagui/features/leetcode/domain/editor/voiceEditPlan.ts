import { normalizeText, type EditorClient, type Range, type Revision, type Step } from '@lib/code-editor-api';
import { applyCommands, type EditAlternative } from '@lib/voice-edit-dsl';

import { planWrite } from './documentWrite';

/** The editor as a plan was made for it: what the planner was shown, and the revision and instance an apply is made at. */
export type EditBasis = { readonly text: string; readonly revision: Revision; readonly instance: string };

export type PlannedEdit = {
  readonly basis: EditBasis;
  readonly steps: readonly Step[];
  readonly modifiedText: string;
  readonly label: string;
};

export type EditRefusal = 'stale' | 'invalid-plan' | 'not-ready' | 'failed';

export type EditOutcome =
  | { readonly ok: true; readonly modifiedText: string; readonly landed: Range | null; readonly revision: Revision }
  | { readonly ok: false; readonly reason: EditRefusal; readonly code: string; readonly message?: string };

export type PlanResult = { readonly ok: true; readonly edit: PlannedEdit } | Extract<EditOutcome, { ok: false }>;

/** What a hook needs of the editor to plan and apply an edit: the handle of the editor view satisfies it. */
export type EditTarget = {
  basis(): Promise<EditBasis | null>;
  applyEdit(edit: PlannedEdit): Promise<EditOutcome>;
};

/** The instance comes first: a page that restarts between the two reads then fails the apply as an instance mismatch. */
export async function readBasis(client: EditorClient): Promise<EditBasis | null> {
  const hello = await client.hello();
  if (!hello.ok || !hello.value.ready) return null;
  const read = await client.getText();
  if (!read.ok) return null;
  return { text: read.value.text, revision: read.revision, instance: hello.value.instance };
}

/**
 * The one-hunk edit an alternative makes to the basis text. The commands' offsets are in the basis text, which is what the planner was
 * shown, so they are applied there; a command set that cannot be applied (two edits over the same span) is a refusal, not a throw.
 */
export function planVoiceEdit(basis: EditBasis, alternative: Pick<EditAlternative, 'commands'>, label = 'voice-edit'): PlanResult {
  let applied: ReturnType<typeof applyCommands>;
  try {
    applied = applyCommands(basis.text, [...alternative.commands]);
  } catch (error) {
    return { ok: false, reason: 'invalid-plan', code: 'invalid-plan', message: error instanceof Error ? error.message : String(error) };
  }
  const selection = alternative.commands.length > 0 ? applied.selection : undefined;
  return { ok: true, edit: { basis, steps: planWrite(basis.text, applied.text, selection), modifiedText: normalizeText(applied.text), label } };
}

const STALE_CODES: ReadonlySet<string> = new Set(['stale', 'instance-mismatch']);

/** One call at the basis revision and instance: a keystroke or a restart since the basis was read refuses it, with nothing applied and no undo step. */
export async function applyPlannedEdit(client: EditorClient, edit: PlannedEdit): Promise<EditOutcome> {
  const reply = await client.transact(edit.steps, { at: edit.basis.revision, instance: edit.basis.instance, stale: 'reject', label: edit.label });
  if (!reply.ok) {
    const code = reply.error.code;
    return { ok: false, reason: STALE_CODES.has(code) ? 'stale' : code === 'not-ready' ? 'not-ready' : 'failed', code, message: reply.error.message };
  }
  const first = edit.steps[0];
  return { ok: true, modifiedText: edit.modifiedText, landed: first?.op === 'replace' ? (reply.value.landed[0] ?? null) : null, revision: reply.revision };
}
