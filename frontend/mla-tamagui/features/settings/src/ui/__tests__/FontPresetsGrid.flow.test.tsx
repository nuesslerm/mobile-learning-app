import {
  FONT_PRESETS,
  useFontPresetStore,
} from '@lib/tamagui-design-system';
import {
  renderScreen,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';

import { FontPresetsGrid } from '../FontPresetsGrid';

// No settings route renders FontPresetsGrid (the Appearance screen shows theme
// tiles only), so there is no path to mount through; render the component.
describe('FontPresetsGrid flow', () => {
  it('given the component renders, when the grid displays, then every FONT_PRESETS entry renders a card', async () => {
    renderScreen(<FontPresetsGrid />);

    await waitFor(() => expect(screen.getByTestId('FontPresetsGrid')).toBeTruthy());

    for (const preset of FONT_PRESETS) {
      const card = screen.getByTestId(`FontCard-${preset.id}`);
      expect(card).toBeTruthy();
      expect(card.textContent).toContain(preset.name);
      expect(card.textContent).toContain(preset.description);
    }
  });

  it('given a font card, when tapped, then it marks the preset as pending and shows the Apply button', async () => {
    renderScreen(<FontPresetsGrid />);

    await waitFor(() => expect(screen.getByTestId('FontPresetsGrid')).toBeTruthy());

    const secondCard = screen.getByTestId(`FontCard-${FONT_PRESETS[1].id}`);
    await userEvent.click(secondCard);

    await waitFor(() => expect(screen.getByTestId('FontPresetsGrid-ApplyBtn')).toBeTruthy());
    const badge = screen.getByTestId(`FontCard-PendingBadge-${FONT_PRESETS[1].id}`);
    expect(badge).toBeTruthy();
  });

  it('given a pending preset, when the Apply button is pressed, then onApplied is called with the selected preset', async () => {
    const onApplied = vi.fn();
    renderScreen(<FontPresetsGrid onApplied={onApplied} />);

    await waitFor(() => expect(screen.getByTestId('FontPresetsGrid')).toBeTruthy());

    const secondCard = screen.getByTestId(`FontCard-${FONT_PRESETS[1].id}`);
    await userEvent.click(secondCard);

    await waitFor(() => expect(screen.getByTestId('FontPresetsGrid-ApplyBtn')).toBeTruthy());

    const applyBtn = screen.getByTestId('FontPresetsGrid-ApplyBtn');
    await userEvent.click(applyBtn);

    expect(onApplied).toHaveBeenCalledWith(FONT_PRESETS[1]);
  });

  it('given a pending preset, when the Apply button is pressed, then the store updates to the selected font and clears pending', async () => {
    renderScreen(<FontPresetsGrid />);

    await waitFor(() => expect(screen.getByTestId('FontPresetsGrid')).toBeTruthy());

    const secondCard = screen.getByTestId(`FontCard-${FONT_PRESETS[1].id}`);
    await userEvent.click(secondCard);

    await waitFor(() => expect(screen.getByTestId('FontPresetsGrid-ApplyBtn')).toBeTruthy());

    const applyBtn = screen.getByTestId('FontPresetsGrid-ApplyBtn');
    await userEvent.click(applyBtn);

    await waitFor(() => {
      const store = useFontPresetStore.getState();
      expect(store.activeFontId).toBe(FONT_PRESETS[1].id);
    });

    const applyRowExists = screen.queryByTestId('FontPresetsGrid-ApplyRow');
    expect(applyRowExists).toBeNull();
  });

  it('given the grid and font cards render, then both surfaces carry non-transparent themed backgrounds', async () => {
    renderScreen(<FontPresetsGrid />);

    await waitFor(() => expect(screen.getByTestId('FontPresetsGrid')).toBeTruthy());

    const grid = screen.getByTestId('FontPresetsGrid');
    const gridBg = getComputedStyle(grid).backgroundColor;
    expect(gridBg).not.toBe('rgba(0, 0, 0, 0)');
    expect(gridBg).not.toBe('transparent');

    const firstCard = screen.getByTestId(`FontCard-Body-${FONT_PRESETS[0].id}`);
    const cardBg = getComputedStyle(firstCard).backgroundColor;
    expect(cardBg).not.toBe('rgba(0, 0, 0, 0)');
    expect(cardBg).not.toBe('transparent');
  });
});
