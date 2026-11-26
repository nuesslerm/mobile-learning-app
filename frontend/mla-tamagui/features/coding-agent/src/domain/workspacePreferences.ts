import { z } from 'zod';

const filters = z.object({
  hideSleepingWorkspaces: z.boolean(),
  alwaysShowDefaultBranchWorkspace: z.boolean(),
  hideDefaultBranchWorkspace: z.boolean(),
  hideAutomationGeneratedWorkspaces: z.boolean(),
  hideCliCreatedWorkspaces: z.boolean(),
  hideDetachedHeadWorkspaces: z.boolean(),
  hideWorkspacesFromOtherDevices: z.boolean(),
  filterRepoIds: z.array(z.string()),
  visibleHostIds: z.array(z.string()).nullable(),
});
export const workspacePreferences = z.object({
  version: z.literal(1),
  preferences: z.object({
    groupBy: z.enum(['repo', 'none', 'workspace-status', 'pr-status']),
    sortBy: z.enum(['name', 'smart', 'recent', 'repo', 'manual']),
    projectOrderBy: z.enum(['manual', 'recent']),
    cardLayout: z.enum(['detailed', 'compact']),
    worktreeCardProperties: z.array(z.string()),
    agentActivityDisplayMode: z.enum(['compact', 'full']),
    filters,
  }),
});

export type WorkspacePreferences = z.infer<
  typeof workspacePreferences
>['preferences'];
export type WorkspacePatch = Partial<Omit<WorkspacePreferences, 'filters'>> & {
  filters?: Partial<WorkspacePreferences['filters']>;
};
