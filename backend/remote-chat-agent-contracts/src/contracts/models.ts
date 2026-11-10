// backend/remote-chat-agent-contracts/src/contracts/models.ts
//
// `GET /api/models` and `GET /api/models/recent` — model list + recency
// proxy. See `backend/remote-chat-agent/rs/src/api/routes/inventory.rs`.
import { z } from 'zod';

export const ModelSchema = z.object({
  id: z.string().min(1),
  providerID: z.string().min(1),
  name: z.string().min(1),
});
export type Model = z.infer<typeof ModelSchema>;

export const ModelsResponseSchema = z.object({
  data: z.array(ModelSchema),
});
export type ModelsResponse = z.infer<typeof ModelsResponseSchema>;

export const RecentModelSchema = ModelSchema.extend({
  count: z.number().int().nonnegative(),
});
export type RecentModel = z.infer<typeof RecentModelSchema>;

export const RecentModelsResponseSchema = z.object({
  data: z.array(RecentModelSchema),
});
export type RecentModelsResponse = z.infer<typeof RecentModelsResponseSchema>;
