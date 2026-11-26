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

export const ORCA_HOME_TABS: readonly HomeTabEntry[] = [
  { id: 'feed', label: 'Feed' },
  { id: 'bots', label: 'Bots' },
  { id: 'agents', label: 'Agents' },
  { id: 'usage', label: 'Usage' },
  { id: 'automations', label: 'Automations' },
];

/** The tab the home opens on. */
export const INITIAL_HOME_TAB: HomeTabId = 'feed';
