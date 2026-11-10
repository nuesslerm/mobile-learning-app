// Stateful scenarios (`mimic-scenarios/1`): ordered requests where a later one names what an earlier answer returned, so a method that needs
// stored state is asked with that state present. Derived from the crawl (`<app>-api/inventory.json` and `corpus.json`): a method whose valid
// payload names an entity (`repo`, `threadId`) gets the chain that creates it; hand-written stories only choose WHICH methods run in which order,
// every payload still comes from the schema-derived corpus. Each case is one `script` input, so one fresh server and profile per scenario.
import type { Json } from '../protocol.ts'
import type { Case } from '../corpus/types.ts'

export type Kind = 'derived' | 'story'
export type Scenario = { id: string; kind: Kind; rows: string[]; note?: string }

/** `populated`: some scenario asks the row with the entity it names present. `empty-only`: only the crawl's empty-state answer exists, for `reason`. */
export type Covered = { row: string; state: 'populated' | 'empty-only'; via: string[]; reason?: string }

export type ScenarioSet = { cases: Case[]; scenarios: Scenario[]; coverage: Covered[] }

export type Endpoint = { id: string; kind: string; name: string; transport: string; streaming: boolean; skipped?: string }
/** What the crawl holds of one row: its valid and boundary payloads, as the app's script op carries them. */
/** `empty`: the schema of the row was unreadable, so the empty payload is the only one the crawl asks. */
export type RowPayloads = { valid?: Json; boundary?: Json; empty?: Json; /** T3: the whole request frame of the valid case (tag, id, headers), so a scenario sends what the crawl sends. */ frame?: Json }
export type Api = { app: 'orca' | 't3'; endpoints: Endpoint[]; payloads: Map<string, RowPayloads> }

export type Summary = { schema: 'mimic-scenarios-summary/1'; app: string; endpoints: number; scenarios: number; populated: number; emptyOnly: number; reasons: Record<string, number>; wrote: string[] }
