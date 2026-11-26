// features/coding-agent/src/state/useHomeNavigation.ts
//
// The state of the Orca home bottom navigation: which tab is active and which
// tabs have been visited. A tab mounts on its first visit and then stays
// mounted, so it keeps its scroll position and its data.

import type { HomeTabId } from '@lib/agent-ui-contracts';
import { useCallback, useState } from 'react';

import {
  INITIAL_HOME_TAB,
  ORCA_HOME_TABS,
  type HomeTabEntry,
} from '../domain/homeNavigation';

export interface HomeNavigation {
  /** The tabs the bar shows, in the order it shows them. */
  entries: readonly HomeTabEntry[];
  /** The tab the bar marks as selected. */
  active: HomeTabId;
  /** The tabs that have been opened; a tab mounts on its first visit. */
  visited: ReadonlySet<HomeTabId>;
  select: (tab: HomeTabId) => void;
}

export function useHomeNavigation(): HomeNavigation {
  const [active, setActive] = useState<HomeTabId>(INITIAL_HOME_TAB);
  const [visited, setVisited] = useState<ReadonlySet<HomeTabId>>(
    () => new Set<HomeTabId>([INITIAL_HOME_TAB])
  );

  const select = useCallback((tab: HomeTabId) => {
    setActive(tab);
    setVisited((current) =>
      current.has(tab) ? current : new Set([...current, tab])
    );
  }, []);

  return { entries: ORCA_HOME_TABS, active, visited, select };
}
