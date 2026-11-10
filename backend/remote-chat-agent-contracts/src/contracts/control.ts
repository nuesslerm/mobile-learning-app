// backend/remote-chat-agent-contracts/src/contracts/control.ts
//
// `POST /api/control/stop|queue|steer` — see
// `backend/remote-chat-agent/rs/src/api/routes/control.rs`.
import { z } from 'zod';

export const ControlActionSchema = z.enum(['stop', 'queue', 'steer']);
export type ControlAction = z.infer<typeof ControlActionSchema>;

/** `POST /api/control/stop` request body. */
export const ControlStopRequestSchema = z.object({
  sessionId: z.string().min(1),
  directory: z.string().min(1),
});
export type ControlStopRequest = z.infer<typeof ControlStopRequestSchema>;

/** `POST /api/control/queue` and `POST /api/control/steer` request body. */
export const ControlMessageRequestSchema = z.object({
  sessionId: z.string().min(1),
  directory: z.string().min(1),
  message: z.string().min(1),
});
export type ControlMessageRequest = z.infer<typeof ControlMessageRequestSchema>;

export const ControlStopResponseSchema = z.object({
  ok: z.literal(true),
  action: z.literal('stop'),
  sessionId: z.string().min(1),
});
export type ControlStopResponse = z.infer<typeof ControlStopResponseSchema>;

export const ControlMessageResponseSchema = z.object({
  ok: z.literal(true),
  action: z.enum(['queue', 'steer']),
  sessionId: z.string().min(1),
  message: z.string().min(1),
});
export type ControlMessageResponse = z.infer<typeof ControlMessageResponseSchema>;
