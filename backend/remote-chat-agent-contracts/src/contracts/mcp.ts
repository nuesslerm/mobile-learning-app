// backend/remote-chat-agent-contracts/src/contracts/mcp.ts
//
// `GET /api/mcp` and `POST /api/mcp/:name/connect|disconnect` — MCP server
// status + toggle proxy, validated against opencode's declared shapes. See `backend/remote-chat-agent/rs/src/api/routes/inventory.rs`. Clients
// build the concrete endpoint by interpolating `connect` or `disconnect`, so
// those are modeled as two concrete paths, not a `{name}/{endpoint}` template.
import { z } from 'zod';

export const McpStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('connected') }),
  z.object({ status: z.literal('disabled') }),
  z.object({ status: z.literal('failed'), error: z.string() }),
  z.object({ status: z.literal('needs_auth') }),
  z.object({ status: z.literal('needs_client_registration'), error: z.string() }),
]);
export type McpStatus = z.infer<typeof McpStatusSchema>;

/** `GET /api/mcp` response: a map of server name -> status. */
export const McpServerMapSchema = z.record(z.string(), McpStatusSchema);
export type McpServerMap = z.infer<typeof McpServerMapSchema>;

/**
 * `POST /api/mcp/:name/connect` and `/disconnect`: opencode answers a bare boolean and the service wraps it as `{ ok }`; any other
 * opencode reply is a 502 `unexpected_mcp_reply` (`backend/remote-chat-agent/rs/src/api/routes/inventory.rs`).
 */
export const McpActionResponseSchema = z.object({
  ok: z.boolean(),
});
export type McpActionResponse = z.infer<typeof McpActionResponseSchema>;
