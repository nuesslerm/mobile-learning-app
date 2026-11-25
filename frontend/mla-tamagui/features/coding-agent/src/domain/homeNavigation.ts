// features/coding-agent/src/domain/homeNavigation.ts
//
// The navigation model of the Orca home bottom navigation: the five tabs the
// bar shows, in the order it shows them. Pure data, no IO: the shell that owns
// the bar reads this table.

import type { HomeTabId } from '@lib/agent-ui-contracts';

export interface HomeTabEntry {
  id: HomeTabId;
  /** The label under the glyph. */
  label: string;
}

/**
 * The tabs of the Orca home, in the order its bar shows them.
 *
 * The original shows five; the staged home shows the same five, so the bar and
 * the screens under it match what the original shows.
 */
export const ORCA_HOME_TABS: readonly HomeTabEntry[] = [
  { id: 'feed', label: 'Feed' },
  { id: 'agents', label: 'Agents' },
  { id: 'usage', label: 'Usage' },
  { id: 'automations', label: 'Automations' },
  { id: 'settings', label: 'Settings' },
];

/** The tab the home opens on. */
export const INITIAL_HOME_TAB: HomeTabId = 'feed';
