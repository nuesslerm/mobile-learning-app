import * as orca from '@lib/orca-skin';
import * as tamagui from '@lib/tamagui-skin';
import { describe, expect, it } from 'vitest';

const orcaEntries: Record<string, unknown> = { ...orca.orcaSkin };
const tamaguiEntries: Record<string, unknown> = { ...tamagui.tamaguiSkin };

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

  it('Given the Tamagui skin, when its entries are compared with the Orca skin, then it borrows none', () => {
    const borrowed = Object.entries(tamaguiEntries)
      .filter(([entry, component]) => component === orcaEntries[entry])
      .map(([entry]) => entry)
      .sort();

    expect(borrowed).toEqual([]);
  });
});
