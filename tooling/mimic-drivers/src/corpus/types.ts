import type { Json } from '../protocol.ts'

export type Case = { id: string; row: string; op: 'script'; input: Json; tags: string[] }

/** What became of one enumerated row: a case in the corpus, or skipped with the reason it cannot be recorded. */
export type Disposition = { row: string; status: 'case' | 'skipped'; case?: string; input?: string; reason?: string }
