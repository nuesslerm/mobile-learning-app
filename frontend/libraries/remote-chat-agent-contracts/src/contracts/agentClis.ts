import { z } from 'zod';

/**
 * One coding-agent CLI backend as reported by `GET /api/agent-clis`.
 *
 * `available` is the load-bearing field: a backend can be *supported* by the
 * service (it appears in this list) while not being *usable* in the running
 * image, because its dependency was pruned at build time via the Dockerfile's
 * `AGENT_CLIS` arg. The server refuses to resolve an unavailable backend, so a
 * client MUST filter on `available` before offering it as a choice — otherwise
 * it presents an option that can only produce an error.
 */
export const AgentCliInfoSchema = z
  .object({
    /** Stable id, also the value to send as the backend selector. */
    id: z.string(),
    /** Human-readable, suitable for a picker row. */
    description: z.string(),
    /** Whether this backend's dependency is actually installed in this image. */
    available: z.boolean(),
    /** Whether this is the backend the server is currently configured to use. */
    selected: z.boolean(),
  })
  .passthrough();
export type AgentCliInfo = z.infer<typeof AgentCliInfoSchema>;

export const AgentClisResponseSchema = z.object({
  data: z.array(AgentCliInfoSchema),
});
export type AgentClisResponse = z.infer<typeof AgentClisResponseSchema>;
