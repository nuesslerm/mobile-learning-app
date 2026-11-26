import { z } from 'zod';

export const checkoutRow = z.object({
  id: z.string(), name: z.string(), branch: z.string().nullable(), main: z.boolean(),
  automaticName: z.string().optional(), pinned: z.boolean().optional(),
});
export type CheckoutRow = z.infer<typeof checkoutRow>;
export type CheckoutEdit = { displayName: string } | { isPinned: boolean };

export const workspaceCatalog = z.object({
  host: z.object({ name: z.string(), os: z.string() }),
  repo: z.string(),
  checkouts: z.array(checkoutRow),
});
export const checkoutSessions = z.object({
  active: z.string().nullable(),
  sessions: z.array(z.object({ id: z.string(), title: z.string() })),
});

export type WorkspaceCatalog = z.infer<typeof workspaceCatalog>;
export type CheckoutSessions = z.infer<typeof checkoutSessions>;

export function chatRoute(thread: string, checkout?: string) {
  const route = '/coding-agent/' + encodeURIComponent(thread);
  return checkout ? route + '?checkout=' + encodeURIComponent(checkout) : route;
}
