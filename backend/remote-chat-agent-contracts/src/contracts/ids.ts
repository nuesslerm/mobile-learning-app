// backend/remote-chat-agent-contracts/src/contracts/ids.ts
//
// Branded identifier schemas for the remote-chat-agent provider runtime.
// Brands keep ThreadId, TurnId, … from being interchangeable in call sites
// without forcing an extra runtime check; `.min(1)` rejects whitespace-only
// ids at the boundary instead of inside the projector.
import { z } from 'zod';

export const ThreadId = z.string().min(1).brand<'ThreadId'>();
export type ThreadId = z.infer<typeof ThreadId>;

export const TurnId = z.string().min(1).brand<'TurnId'>();
export type TurnId = z.infer<typeof TurnId>;

export const MessageId = z.string().min(1).brand<'MessageId'>();
export type MessageId = z.infer<typeof MessageId>;

export const EventId = z.string().min(1).brand<'EventId'>();
export type EventId = z.infer<typeof EventId>;

export const CommandId = z.string().min(1).brand<'CommandId'>();
export type CommandId = z.infer<typeof CommandId>;

export const ApprovalRequestId = z.string().min(1).brand<'ApprovalRequestId'>();
export type ApprovalRequestId = z.infer<typeof ApprovalRequestId>;

export const ProviderItemId = z.string().min(1).brand<'ProviderItemId'>();
export type ProviderItemId = z.infer<typeof ProviderItemId>;

export const IsoDateTime = z.string().datetime();
export type IsoDateTime = z.infer<typeof IsoDateTime>;
