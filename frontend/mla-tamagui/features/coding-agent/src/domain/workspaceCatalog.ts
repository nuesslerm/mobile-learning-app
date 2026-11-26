import { z } from 'zod';

export const checkoutRow = z.object({
  id: z.string(), name: z.string(), branch: z.string().nullable(), main: z.boolean(),
  automaticName: z.string().optional(), pinned: z.boolean().optional(),
});
export type CheckoutRow = z.infer<typeof checkoutRow>;
export type CheckoutEdit = { displayName: string } | { isPinned: boolean };

const desktopHost = z.object({ name: z.string(), os: z.string() });
export const homeSummary = z.object({
  host: desktopHost,
  worktreeCount: z.number().int().nonnegative(),
});
export type HomeSummary = z.infer<typeof homeSummary>;

export const workspaceCatalog = z.object({
  host: desktopHost,
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

const platformNames: Record<string, string> = {
  darwin: 'macOS', macos: 'macOS', linux: 'Linux', win32: 'Windows',
};

export function desktopPlatform(os: string) {
  return platformNames[os] ?? os;
}

export function desktopSummary(home: HomeSummary) {
  const count = home.worktreeCount;
  const worktrees = count + (count === 1 ? ' worktree' : ' worktrees');
  return [worktrees, desktopPlatform(home.host.os)].filter(Boolean).join(' · ');
}
