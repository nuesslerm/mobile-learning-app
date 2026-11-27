import * as orca from '@lib/orca-skin';
import * as tamagui from '@lib/tamagui-skin';
import { describe, expect, it } from 'vitest';

const orcaEntries: Record<string, unknown> = { ...orca.orcaSkin };
const tamaguiEntries: Record<string, unknown> = { ...tamagui.tamaguiSkin };

// The entries the Tamagui skin has not built yet and still takes from the Orca skin. This list only shrinks.
const BORROWED_FROM_ORCA = Object.keys(orca.orcaSkin)
  .filter((entry) => !['HomeSurface', 'HomeTabBar', 'HomeTabHeader', 'HomeEmptyState', 'HomeSearchField', 'HomeAgentSection', 'HomeActionSheet', 'BrandMark', 'PairingScreen', 'PairingCodeSheet', 'PairedDesktops', 'PairDesktopButton', 'UnpairedHome', 'OnboardingChoice', 'NotificationChoices', 'ChatSurface', 'MessageBubble', 'ChatMessageText', 'ChatMessageMeta', 'ChatToolActivity', 'ChatSizeControl', 'ChatSpacingControl', 'Composer', 'ComposerAddButton', 'ComposerSuggestionList', 'ComposerSendButton', 'ComposerOptions', 'SessionTabsSheet', 'NewTabSheet', 'ChatImage', 'ChatImageViewer', 'HomeSettingsGroup', 'HomeSettingsNote', 'HomeSettingsRow', 'HomeSettingsScreen', 'Row', 'Stack', 'Text', 'Button', 'TextField', 'Switch', 'Spinner', 'Notice', 'HomeSettingsSectionTitle', 'HomeSettingsSpacer', 'ChatTextPreview', 'OptionSheet'].includes(entry))
  .sort();

describe('The two skins', () => {
  it('Given a skin package, when it is imported, then it exposes its skin and its theme provider and nothing else', () => {
    expect(Object.keys(orca).sort()).toEqual(['OrcaThemeProvider', 'orcaSkin']);
    expect(Object.keys(tamagui).sort()).toEqual(['TamaguiThemeProvider', 'tamaguiSkin']);
  });

  it('Given both skins, when their entries are listed, then each implements every entry of the contract', () => {
    const entries = Object.keys(orca.orcaSkin).sort();

    expect(Object.keys(tamagui.tamaguiSkin).sort()).toEqual(entries);
    for (const skin of [orcaEntries, tamaguiEntries]) {
      expect(entries.filter((entry) => typeof skin[entry] !== 'function' && typeof skin[entry] !== 'object')).toEqual([]);
    }
  });

  it('Given the Tamagui skin, when its entries are compared with the Orca skin, then only the listed ones are still borrowed', () => {
    const borrowed = Object.entries(tamaguiEntries)
      .filter(([entry, component]) => component === orcaEntries[entry])
      .map(([entry]) => entry)
      .sort();

    expect(borrowed).toEqual(BORROWED_FROM_ORCA);
  });
});
