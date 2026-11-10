// features/coding-agent/src/domain/modelOptions.ts
//
// Pure derivation of the model picker rows shown in the thread settings sheet.
// Ported from references/t3code/apps/mobile/src/lib/modelOptions.ts
// `buildModelOptions` / `groupByProvider` over our `server.probe` shape
// (`{availableClis, selectedCli, models?}`): no `effect`, no IO. The probe
// never carries the auth/install state the t3code reference required, so
// availability comes from the probe's `availableClis` directly. A
// `currentSelection` that isn't listed in `probe.models` (typically a legacy
// model or a stale persisted choice) is appended as a selectable option so the
// user can keep using it; it is flagged `isLegacy: true` so the picker can
// label it.

import { ProviderName } from '@app/remote-chat-agent-contracts';
import type { ModelSelection } from '../contracts/orchestration';

export interface ServerProbeModelDescriptor {
  readonly modelID: string;
  readonly name: string;
  readonly providerID?: string;
  readonly isDefault?: boolean;
  readonly isLegacy?: boolean;
}

export interface ServerProbeSummary {
  readonly availableClis: ReadonlyArray<ProviderName>;
  readonly selectedCli: ProviderName | null;
  readonly models?: Partial<Record<ProviderName, ReadonlyArray<ServerProbeModelDescriptor>>>;
}

interface ModelOption {
  readonly key: string;
  readonly cli: ProviderName;
  readonly cliLabel: string;
  readonly modelID: string;
  readonly providerID?: string;
  readonly label: string;
  readonly subtitle: string;
  readonly isDefault: boolean;
  readonly isLegacy: boolean;
  readonly isCurrent: boolean;
  readonly selection: ModelSelection;
}

export interface CliModelGroup {
  readonly cli: ProviderName;
  readonly cliLabel: string;
  readonly isAvailable: boolean;
  readonly isSelected: boolean;
  readonly options: ReadonlyArray<ModelOption>;
}

function cliLabel(cli: ProviderName): string {
  switch (cli) {
    case 'opencode':
      return 'OpenCode';
    case 'claude':
      return 'Claude';
    case 'codex':
      return 'Codex';
  }
}

function modelKey(
  cli: ProviderName,
  modelID: string,
  providerID?: string,
): string {
  return providerID !== undefined && providerID.length > 0
    ? `${cli}:${providerID}:${modelID}`
    : `${cli}:${modelID}`;
}

function modelSelection(
  cli: ProviderName,
  modelID: string,
  providerID?: string,
): ModelSelection {
  return providerID !== undefined && providerID.length > 0
    ? { cli, providerID, modelID }
    : { cli, modelID };
}

function asTrimmedString(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function modelOptionFromDescriptor(
  cli: ProviderName,
  descriptor: ServerProbeModelDescriptor,
): ModelOption | null {
  const modelID = asTrimmedString(descriptor.modelID);
  if (modelID === null) {
    return null;
  }
  const providerID = asTrimmedString(descriptor.providerID) ?? undefined;
  const name = asTrimmedString(descriptor.name) ?? modelID;
  return {
    key: modelKey(cli, modelID, providerID),
    cli,
    cliLabel: cliLabel(cli),
    modelID,
    ...(providerID !== undefined ? { providerID } : {}),
    label: name,
    subtitle: cliLabel(cli),
    isDefault: descriptor.isDefault === true,
    isLegacy: descriptor.isLegacy === true,
    isCurrent: false,
    selection: modelSelection(cli, modelID, providerID),
  };
}

function currentSelectionKey(selection: ModelSelection): string {
  return modelKey(selection.cli, selection.modelID ?? '', selection.providerID);
}

function ensureGroup(
  groups: Map<ProviderName, Map<string, ModelOption>>,
  cli: ProviderName,
): Map<string, ModelOption> {
  let bucket = groups.get(cli);
  if (bucket === undefined) {
    bucket = new Map();
    groups.set(cli, bucket);
  }
  return bucket;
}

/**
 * Build the per-CLI option groups for the model picker. Every CLI in
 * `probe.availableClis`, the probe's `selectedCli`, and `currentSelection.cli`
 * appears as a group (so the picker can still show what the thread is bound
 * to). `isAvailable` reflects whether the CLI is in `availableClis`; groups
 * for unavailable CLIs surface only the legacy current selection if any. The
 * `currentSelection` (when it has a modelID) is always added to its group's
 * options even if the probe doesn't include it — the picker can offer it as
 * a "keep using" choice.
 */
export function buildModelOptions(
  probe: ServerProbeSummary,
  currentSelection: ModelSelection | null,
): CliModelGroup[] {
  const availableClis = new Set<ProviderName>(probe.availableClis);
  const buckets = new Map<ProviderName, Map<string, ModelOption>>();

  for (const cli of Object.keys(probe.models ?? {}) as ProviderName[]) {
    const bucket = ensureGroup(buckets, cli);
    for (const descriptor of probe.models?.[cli] ?? []) {
      const option = modelOptionFromDescriptor(cli, descriptor);
      if (option !== null) {
        bucket.set(option.key, option);
      }
    }
  }

  if (probe.selectedCli !== null) {
    ensureGroup(buckets, probe.selectedCli);
  }

  if (currentSelection !== null && currentSelection.modelID !== undefined) {
    const bucket = ensureGroup(buckets, currentSelection.cli);
    const key = currentSelectionKey(currentSelection);
    const existing = bucket.get(key);
    if (existing !== undefined) {
      bucket.set(key, {
        ...existing,
        isCurrent: true,
        selection: currentSelection,
      });
    } else {
      bucket.set(key, {
        key,
        cli: currentSelection.cli,
        cliLabel: cliLabel(currentSelection.cli),
        modelID: currentSelection.modelID,
        ...(currentSelection.providerID !== undefined
          ? { providerID: currentSelection.providerID }
          : {}),
        label: currentSelection.modelID,
        subtitle: cliLabel(currentSelection.cli),
        isDefault: false,
        isLegacy: true,
        isCurrent: true,
        selection: currentSelection,
      });
    }
  }

  const groups: CliModelGroup[] = [];
  for (const [cli, bucket] of buckets) {
    groups.push({
      cli,
      cliLabel: cliLabel(cli),
      isAvailable: availableClis.has(cli),
      isSelected: probe.selectedCli === cli,
      options: [...bucket.values()],
    });
  }
  return groups;
}