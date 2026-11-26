import { z } from 'zod';
import type { T3ComposerEntry } from '../../domain/t3Mobile';

type Call = (method: string, payload: unknown) => Promise<unknown>;

export async function composerFiles(
  call: Call,
  cwd: string,
  query: string
): Promise<T3ComposerEntry[]> {
  const { entries } = z
    .object({
      entries: z.array(z.object({ path: z.string(), kind: z.string() })),
    })
    .parse(
      await call('projects.searchEntries', {
        cwd,
        query,
        kind: 'file',
        limit: 10,
      })
    );
  return entries
    .filter((row) => row.kind === 'file')
    .map((row) => ({ kind: 'file', token: row.path }));
}

const lists = z.object({
  slashCommands: z
    .array(
      z.object({
        name: z.string(),
        description: z.string().optional(),
        input: z.object({ hint: z.string() }).optional(),
      })
    )
    .optional(),
  skills: z
    .array(
      z.object({
        name: z.string(),
        description: z.string().optional(),
        shortDescription: z.string().optional(),
        enabled: z.boolean(),
        userInvocable: z.boolean().optional(),
      })
    )
    .optional(),
});
const config = z.object({
  providers: z.array(
    lists.extend({
      instanceId: z.string(),
      driver: z.string().optional(),
      workspaceSnapshots: z.array(lists.extend({ cwd: z.string() })).optional(),
    })
  ),
});

export async function composerCatalog(
  call: Call,
  cwd: string,
  instanceId: string
): Promise<T3ComposerEntry[]> {
  const result = config.parse(
    await call('server.refreshProviders', {
      cwd,
      instanceId,
      workspaceSnapshotsOnDemand: true,
      providerCatalogsOnDemand: true,
    })
  );
  const provider = result.providers.find(
    (row) => row.instanceId === instanceId
  );
  if (!provider) return [];
  const source =
    provider.workspaceSnapshots?.find((row) => row.cwd === cwd) ?? provider;
  const prefix = (provider.driver ?? instanceId) === 'codex' ? '$' : '/';
  return [
    ...(source.slashCommands ?? []).map((row) => ({
      kind: 'command' as const,
      token: '/' + row.name.replace(/^\//, ''),
      description: row.description,
      hint: row.input?.hint,
    })),
    ...(source.skills ?? [])
      .filter((row) => row.enabled && row.userInvocable !== false)
      .map((row) => ({
        kind: 'skill' as const,
        token: prefix + row.name.replace(/^[$/]/, ''),
        description: row.shortDescription ?? row.description,
      })),
  ];
}
