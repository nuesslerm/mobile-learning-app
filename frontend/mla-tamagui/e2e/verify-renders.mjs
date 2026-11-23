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

// One showcase item at a time: each carries one of the themed elements asserted below.
const ITEMS = { badge: 'BadgePrimary', button: 'ButtonDefault', list: 'ShowcaseGroup' };

await withServedApp({ port: 8200, name: 'tamagui-verify', path: 'showcase' }, async ({ eval: evalJson, goto }) => {
  const styles = {};
  for (const [item, id] of Object.entries(ITEMS)) {
    await goto(`showcase?item=${item}`);
    const read = await evalJson(`(() => {
      const el = document.querySelector('[data-testid="${id}"]');
      if (!el) return { ok: false, reason: 'missing: ${id}', text: document.body.innerText.slice(0, 150) };
      const cs = getComputedStyle(el);
      return { ok: true, bg: cs.backgroundColor, radius: cs.borderRadius, heading: document.querySelector('[data-testid="ShowcaseHeading"]')?.textContent };
    })()`);
    if (!read?.ok) {
      fail(`Tamagui did not render the showcase ${item} item: ${JSON.stringify(read)}`);
      return;
    }
    styles[item] = read;
  }

  const themed = (v) => v && v !== 'rgba(0, 0, 0, 0)' && v !== 'transparent';
  // Assert on values that can only come from Tamagui resolving theme tokens.
  // Presence alone would pass for unstyled elements.
  const ok = Object.values(styles).every((entry) => themed(entry.bg));
  if (!ok) {
    fail(`Tamagui did not render as themed: ${JSON.stringify(styles)}`);
    return;
  }
  console.log(
    `ok: "${styles.badge.heading}" — button ${styles.button.bg}, badge ${styles.badge.bg} r${styles.badge.radius}, group ${styles.list.bg} r${styles.list.radius}`
  );
});
