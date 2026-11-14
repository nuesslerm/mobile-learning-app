// frontend/libraries/remote-chat-agent-contracts/src/contracts/health.ts
//
// `GET /health` — see `backend/remote-chat-agent/rs/src/api`.
import { z } from 'zod';

export const HealthResponseSchema = z.object({
  status: z.literal('ok'),
});
export type HealthResponse = z.infer<typeof HealthResponseSchema>;
