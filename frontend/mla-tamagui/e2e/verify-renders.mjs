/**
 * Phase-5 acceptance (#213): prove Tamagui actually RENDERS, not just bundles.
 *
 * A passing `expo export` only shows the graph resolved. This asserts on
 * computed styles read out of the live DOM -- a Tamagui theme token reaching a
 * real element is the thing that would break if the config, the babel plugin
 * or the metro resolver were misconfigured, and none of that shows up in a
 * bundle's exit code.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

await withServedApp({ port: 8200, name: 'tamagui-verify', path: 'showcase' }, async ({ eval: evalJson }) => {
  const result = await evalJson(`(() => {
    const q = (id) => document.querySelector('[data-testid="' + id + '"]');
    const need = ['ShowcaseHeading','BadgePrimary','ButtonDefault','ShowcaseNameField','ShowcaseGroup','RowSettings','ShowcaseDestructive'];
    const missing = need.filter((id) => !q(id));
    if (missing.length) return { ok: false, reason: 'missing: ' + missing.join(','), text: document.body.innerText.slice(0, 150) };

    const cs = (id) => getComputedStyle(q(id));
    const btn = cs('ButtonDefault');
    const badge = cs('BadgePrimary');
    const group = cs('ShowcaseGroup');
    const themed = (v) => v && v !== 'rgba(0, 0, 0, 0)' && v !== 'transparent';

    // Assert on values that can only come from Tamagui resolving theme tokens.
    // Presence alone would pass for unstyled elements.
    return {
      ok: themed(btn.backgroundColor) && themed(badge.backgroundColor) && themed(group.backgroundColor),
      heading: q('ShowcaseHeading').textContent,
      buttonBg: btn.backgroundColor,
      badgeBg: badge.backgroundColor,
      badgeRadius: badge.borderRadius,
      groupBg: group.backgroundColor,
      groupRadius: group.borderRadius,
    };
  })()`);

  if (!result?.ok) {
    fail(`Tamagui did not render as themed: ${JSON.stringify(result)}`);
    return;
  }
  console.log(
    `ok: "${result.heading}" — button ${result.buttonBg}, badge ${result.badgeBg} r${result.badgeRadius}, group ${result.groupBg} r${result.groupRadius}`
  );
});
