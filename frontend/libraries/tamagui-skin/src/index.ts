import type { Skin } from '@lib/skin';
import { orcaSkin } from '@lib/orca-skin';

import { HomeSettingsGroup } from './adapters/home/HomeSettingsGroup';
import { HomeSettingsNote } from './adapters/home/HomeSettingsNote';
import { HomeSettingsRow } from './adapters/home/HomeSettingsRow';
import { HomeSettingsScreen } from './adapters/home/HomeSettingsScreen';
import { HomeSurface } from './adapters/home/HomeSurface';
import { Row, Stack, Text } from './adapters/layout/layout';
import { Button, Notice, Spinner, Switch, TextField } from './adapters/controls/controls';

// Starts as the Orca set; each area replaces its entries with Tamagui ones.
export const tamaguiSkin: Skin = {
  ...orcaSkin,
  HomeSurface,
  HomeSettingsGroup,
  HomeSettingsNote,
  HomeSettingsRow,
  HomeSettingsScreen,
  Row,
  Stack,
  Text,
  Button,
  TextField,
  Switch,
  Spinner,
  Notice,
};

export { TamaguiThemeProvider } from './adapters/theme/TamaguiThemeProvider';
