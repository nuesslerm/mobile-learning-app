import { GALLERY_ENTRIES, GALLERY_SECTIONS } from '@lib/skin';
import { screen, waitFor } from '@lib/flow-test-harness';
import { afterAll, describe, expect, it } from 'vitest';

import { useSkinStore } from '../../app-shell/skin/skinStore';
import { app } from './app';

describe.each(['tamagui', 'orca'] as const)('The skin gallery under the %s skin', (skin) => {
  afterAll(() => {
    useSkinStore.setState({ skin: 'tamagui' });
  });

  it.each(['index', ...GALLERY_SECTIONS])('Given the %s section, when it opens, then the skin draws it', async (section) => {
    useSkinStore.setState({ skin });

    await app.render('/gallery', section === 'index' ? {} : { search: { section } });

    await waitFor(() => expect(screen.getByTestId(`SkinGallery-${section}`)).toBeTruthy());
  });
});

describe('The gallery and the contract', () => {
  it('Given the contract, when its entries are listed, then every one has a section in the gallery', () => {
    expect(Object.values(GALLERY_ENTRIES).every((section) => GALLERY_SECTIONS.includes(section))).toBe(true);
    expect(Object.keys(GALLERY_ENTRIES).length).toBeGreaterThanOrEqual(42);
  });
});
