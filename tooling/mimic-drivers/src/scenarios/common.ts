import type { Api, Covered, Endpoint } from './types.ts'

export function fail(message: string): never {
  throw new Error(`scenarios: ${message}`)
}

/** Every endpoint once: populated when a scenario asks it, otherwise empty-only with the reason `why` gives. */
export function coverageOf(api: Api, via: Map<string, string[]>, why: (e: Endpoint) => string): Covered[] {
  return api.endpoints.map((e) => {
    const ids = via.get(e.id)
    return ids?.length ? { row: e.id, state: 'populated', via: ids } : { row: e.id, state: 'empty-only', via: [], reason: why(e) }
  })
}
