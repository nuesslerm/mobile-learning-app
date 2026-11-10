/**
 * u28: prove the re-skinned appearance settings screen RENDERS themed, not just bundles.
 *
 * Same bar as verify-renders.mjs: a Tamagui theme token reaching a real
 * element is the thing that breaks silently when the config, babel plugin or
 * metro resolver drift, and none of that shows up in a bundle's exit code.
 * Here the screen, the System/Light/Dark control and a preset tile must carry
 * themed, non-transparent backgrounds -- presence alone would pass for
 * unstyled elements.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

await withServedApp({ port: 8224, name: 'settings-verify', path: 'settings/appearance' }, async ({ eval: evalJson }) => {
  const result = await evalJson(`(() => {
    const q = (id) => document.querySelector('[data-testid="' + id + '"]');
    const tile = document.querySelector('[data-testid^="AppearanceSettingsScreen-ThemeTilePress-"]');
    const need = ['AppearanceSettingsScreen','AppearanceSettingsScreen-ModeControl','AppearanceSettingsScreen-ThemeGrid'];
    const missing = need.filter((id) => !q(id));
    if (!tile) missing.push('AppearanceSettingsScreen-ThemeTilePress-*');
    if (missing.length) return { ok: false, reason: 'missing: ' + missing.join(','), text: document.body.innerText.slice(0, 150) };

    const cs = (el) => getComputedStyle(el);
    const screen = cs(q('AppearanceSettingsScreen'));
    const toggle = cs(q('AppearanceSettingsScreen-ModeControl'));
    const tileStyle = cs(tile);
    const themed = (v) => v && v !== 'rgba(0, 0, 0, 0)' && v !== 'transparent';

    // Assert on values that can only come from Tamagui resolving theme tokens.
    // The control needs both a background and a radius, and the tile its card
    // surface: a genuinely unstyled screen would pass on presence alone.
    return {
      ok:
        themed(screen.backgroundColor) &&
        themed(toggle.backgroundColor) &&
        toggle.borderRadius !== '0px' &&
        themed(tileStyle.backgroundColor),
      title: document.querySelector('[data-testid="AppearanceSettingsScreenHeaderTitle"]')?.textContent,
      screenBg: screen.backgroundColor,
      toggleBg: toggle.backgroundColor,
      toggleRadius: toggle.borderRadius,
      tileBg: tileStyle.backgroundColor,
    };
  })()`);

  if (!result?.ok) {
    fail(`Settings did not render as themed: ${JSON.stringify(result)}`);
    return;
  }
  console.log(
    `ok: "${result.title}" — screen ${result.screenBg}, mode control ${result.toggleBg} r${result.toggleRadius}, preset tile ${result.tileBg}`
  );
});
