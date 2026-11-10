// features/coding-agent/src/domain/__tests__/modelOptions.test.ts
//
// Ported in structure from
// references/t3code/apps/mobile/src/lib/modelOptions.test.ts onto our
// `ServerProbeSummary` shape and `CliModelGroup` output. The reference also
// covers capability option-descriptor normalization and a
// `resolveDefaultableModelSelection` helper that the t3code picker relies on;
// neither exists here, so those cases are dropped. Every test is
// Given/When/Then. The `RUNTIME_MODE_OPTIONS` / `INTERACTION_MODE_OPTIONS`
// static pickers (from `../threadSettings`) are exercised as a flat list
// because the settings sheet renders one row per entry.

import { describe, expect, it } from 'vitest';

import {
  buildModelOptions,
  type ServerProbeModelDescriptor,
  type ServerProbeSummary,
} from '../modelOptions';
import {
  INTERACTION_MODE_OPTIONS,
  RUNTIME_MODE_OPTIONS,
} from '../threadSettings';
import type { ModelSelection } from '../../contracts/orchestration';

function makeDescriptor(
  overrides: Partial<ServerProbeModelDescriptor>
): ServerProbeModelDescriptor {
  return {
    modelID: 'gpt-test',
    name: 'GPT Test',
    isDefault: false,
    isLegacy: false,
    ...overrides,
  };
}

function makeProbe(
  overrides: Partial<ServerProbeSummary> = {}
): ServerProbeSummary {
  return {
    availableClis: ['codex'],
    selectedCli: 'codex',
    models: {
      codex: [],
    },
    ...overrides,
  };
}

describe('buildModelOptions', () => {
  it('given a probe with one provider, when built, then a single group lists the models with isLegacy flags', () => {
    // Given a probe with a codex provider carrying a non-legacy and a legacy model
    const probe = makeProbe({
      models: {
        codex: [
          makeDescriptor({
            modelID: 'gpt-5.6-sol',
            name: 'GPT-5.6 Sol',
          }),
          makeDescriptor({
            modelID: 'gpt-5.4',
            name: 'GPT-5.4',
            isLegacy: true,
          }),
        ],
      },
    });
    // When built
    const groups = buildModelOptions(probe, null);
    // Then a single codex group lists both models with isLegacy flags
    expect(groups).toHaveLength(1);
    expect(groups[0]).toMatchObject({
      cli: 'codex',
      cliLabel: 'Codex',
      isAvailable: true,
      isSelected: true,
    });
    expect(groups[0]?.options).toEqual([
      expect.objectContaining({
        key: 'codex:gpt-5.6-sol',
        label: 'GPT-5.6 Sol',
        isLegacy: false,
        isCurrent: false,
      }),
      expect.objectContaining({
        key: 'codex:gpt-5.4',
        label: 'GPT-5.4',
        isLegacy: true,
        isCurrent: false,
      }),
    ]);
  });

  it('given a current selection whose CLI is not in availableClis, when built, then the group is unavailable but still lists the current model as a legacy option', () => {
    // Given a probe where codex is unavailable but the current selection is bound to codex
    const probe = makeProbe({
      availableClis: ['claude'],
      selectedCli: 'claude',
      models: {
        claude: [],
      },
    });
    const currentSelection: ModelSelection = {
      cli: 'codex',
      modelID: 'gpt-5.6-sol',
    };
    // When built
    const groups = buildModelOptions(probe, currentSelection);
    // Then the codex group is unavailable but still carries the current model as legacy
    const codexGroup = groups.find((group) => group.cli === 'codex');
    expect(codexGroup).toMatchObject({
      cli: 'codex',
      cliLabel: 'Codex',
      isAvailable: false,
      isSelected: false,
    });
    expect(codexGroup?.options).toEqual([
      expect.objectContaining({
        key: 'codex:gpt-5.6-sol',
        modelID: 'gpt-5.6-sol',
        isLegacy: true,
        isCurrent: true,
        selection: currentSelection,
      }),
    ]);
  });

  it('given a current selection that already exists in the probe, when built, then the existing option is marked current and not duplicated', () => {
    // Given a probe listing the same model the current selection points at
    const probe = makeProbe({
      models: {
        codex: [
          makeDescriptor({
            modelID: 'gpt-5.6-sol',
            name: 'GPT-5.6 Sol',
            isDefault: true,
          }),
        ],
      },
    });
    const currentSelection: ModelSelection = {
      cli: 'codex',
      modelID: 'gpt-5.6-sol',
    };
    // When built
    const groups = buildModelOptions(probe, currentSelection);
    // Then the group has exactly one option flagged as current and the default flag is preserved
    const codexGroup = groups.find((group) => group.cli === 'codex');
    expect(codexGroup?.options).toHaveLength(1);
    expect(codexGroup?.options[0]).toMatchObject({
      key: 'codex:gpt-5.6-sol',
      isDefault: true,
      isCurrent: true,
      isLegacy: false,
      selection: currentSelection,
    });
  });

  it('given a current selection with a providerID, when built, then the option key and selection carry the providerID', () => {
    // Given a probe with no models listed and a providerID-bearing current selection
    const probe = makeProbe({ models: { codex: [] } });
    const currentSelection: ModelSelection = {
      cli: 'codex',
      providerID: 'azure',
      modelID: 'gpt-5.6-sol',
    };
    // When built
    const groups = buildModelOptions(probe, currentSelection);
    // Then the only option keys the providerID and the selection round-trips
    const codexGroup = groups.find((group) => group.cli === 'codex');
    expect(codexGroup?.options[0]).toMatchObject({
      key: 'codex:azure:gpt-5.6-sol',
      providerID: 'azure',
      modelID: 'gpt-5.6-sol',
      selection: currentSelection,
    });
  });
});

describe('RUNTIME_MODE_OPTIONS', () => {
  it('given the static runtime-mode picker, when read, then every entry exposes a label and a distinct value', () => {
    // Given the four runtime modes declared in the schema
    // When read
    const values = RUNTIME_MODE_OPTIONS.map((option) => option.mode);
    // Then each enum value appears exactly once and every option carries a label
    expect(RUNTIME_MODE_OPTIONS).toHaveLength(4);
    expect(new Set(values)).toEqual(
      new Set(['approval-required', 'auto-accept-edits', 'auto', 'full-access'])
    );
    for (const option of RUNTIME_MODE_OPTIONS) {
      expect(option.label.length).toBeGreaterThan(0);
    }
  });

  it('given the approval-required entry, when asserted, then its label and value match the schema', () => {
    // Given the approval-required runtime mode
    const option = RUNTIME_MODE_OPTIONS.find(
      (entry) => entry.mode === 'approval-required'
    );
    // When asserted
    // Then its label and value match the schema
    expect(option).toMatchObject({
      mode: 'approval-required',
      label: 'Supervised',
    });
  });

  it('given the auto-accept-edits entry, when asserted, then its label and value match the schema', () => {
    // Given the auto-accept-edits runtime mode
    const option = RUNTIME_MODE_OPTIONS.find(
      (entry) => entry.mode === 'auto-accept-edits'
    );
    // When asserted
    // Then its label and value match the schema
    expect(option).toMatchObject({
      mode: 'auto-accept-edits',
      label: 'Auto-accept edits',
    });
  });

  it('given the auto entry, when asserted, then its label and value match the schema', () => {
    // Given the auto runtime mode
    const option = RUNTIME_MODE_OPTIONS.find((entry) => entry.mode === 'auto');
    // When asserted
    // Then its label and value match the schema
    expect(option).toMatchObject({
      mode: 'auto',
      label: 'Auto',
    });
  });

  it('given the full-access entry, when asserted, then its label and value match the schema', () => {
    // Given the full-access runtime mode
    const option = RUNTIME_MODE_OPTIONS.find(
      (entry) => entry.mode === 'full-access'
    );
    // When asserted
    // Then its label and value match the schema
    expect(option).toMatchObject({
      mode: 'full-access',
      label: 'Full access',
    });
  });
});

describe('INTERACTION_MODE_OPTIONS', () => {
  it('given the static interaction-mode picker, when read, then every entry exposes a label and a distinct value', () => {
    // Given the two interaction modes declared in the schema
    // When read
    const values = INTERACTION_MODE_OPTIONS.map((option) => option.mode);
    // Then each enum value appears exactly once and every option carries a label
    expect(INTERACTION_MODE_OPTIONS).toHaveLength(2);
    expect(new Set(values)).toEqual(new Set(['default', 'plan']));
    for (const option of INTERACTION_MODE_OPTIONS) {
      expect(option.label.length).toBeGreaterThan(0);
    }
  });

  it('given the default entry, when asserted, then its label and value match the schema', () => {
    // Given the default interaction mode
    const option = INTERACTION_MODE_OPTIONS.find(
      (entry) => entry.mode === 'default'
    );
    // When asserted
    // Then its label and value match the schema
    expect(option).toMatchObject({
      mode: 'default',
      label: 'Build',
    });
  });

  it('given the plan entry, when asserted, then its label and value match the schema', () => {
    // Given the plan interaction mode
    const option = INTERACTION_MODE_OPTIONS.find(
      (entry) => entry.mode === 'plan'
    );
    // When asserted
    // Then its label and value match the schema
    expect(option).toMatchObject({
      mode: 'plan',
      label: 'Plan',
    });
  });
});
