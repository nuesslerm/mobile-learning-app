import type { AgentComponents } from '@lib/agent-ui-contracts';
import { orcaComponents } from '@lib/orca-design-system';

import { HomeSettingsGroup } from './adapters/home/HomeSettingsGroup';
import { HomeSettingsNote } from './adapters/home/HomeSettingsNote';
import { HomeSettingsRow } from './adapters/home/HomeSettingsRow';
import { HomeSettingsScreen } from './adapters/home/HomeSettingsScreen';
import { HomeSurface } from './adapters/home/HomeSurface';

// Starts as the Orca set; each area replaces its entries with Tamagui ones.
export const tamaguiComponents: AgentComponents = {
  ...orcaComponents,
  HomeSurface,
  HomeSettingsGroup,
  HomeSettingsNote,
  HomeSettingsRow,
  HomeSettingsScreen,
};

export { TamaguiThemeProvider } from './adapters/theme/TamaguiThemeProvider';
