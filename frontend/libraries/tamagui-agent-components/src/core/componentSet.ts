import type { AgentComponents } from '@lib/agent-ui-contracts';
import { OrcaThemeProvider, orcaComponents } from '@lib/orca-design-system';

// Starts as the Orca set; each area replaces its entries with Tamagui ones.
export const tamaguiComponents: AgentComponents = { ...orcaComponents };

export const TamaguiThemeProvider = OrcaThemeProvider;
