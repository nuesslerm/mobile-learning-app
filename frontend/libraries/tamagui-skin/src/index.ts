import type { Skin } from '@lib/skin';
import { orcaSkin } from '@lib/orca-skin';

import { HomeSettingsGroup } from './adapters/home/HomeSettingsGroup';
import { HomeSettingsNote } from './adapters/home/HomeSettingsNote';
import { HomeSettingsRow } from './adapters/home/HomeSettingsRow';
import { HomeSettingsScreen } from './adapters/home/HomeSettingsScreen';
import { HomeSurface } from './adapters/home/HomeSurface';
import { HomeTabBar } from './adapters/home/HomeTabBar';
import { HomeTabHeader } from './adapters/home/HomeTabHeader';
import { HomeActionSheet, HomeAgentSection, HomeEmptyState, HomeSearchField } from './adapters/home/homeTabExtras';
import { ChatTextPreview, HomeSettingsSectionTitle, HomeSettingsSpacer, OptionSheet } from './adapters/home/settingsExtras';
import { Row, Stack, Text } from './adapters/layout/layout';
import { BrandMark } from './adapters/pairing/BrandMark';
import { NotificationChoices, OnboardingChoice, PairDesktopButton, PairedDesktops, PairingCodeSheet, PairingScreen, UnpairedHome } from './adapters/pairing/pairing';
import { Button, Notice, Spinner, Switch, TextField } from './adapters/controls/controls';

// Starts as the Orca set; each area replaces its entries with Tamagui ones.
export const tamaguiSkin: Skin = {
  ...orcaSkin,
  HomeSurface,
  HomeTabBar,
  HomeTabHeader,
  HomeEmptyState,
  HomeSearchField,
  HomeAgentSection,
  HomeActionSheet,
  BrandMark,
  PairingScreen,
  PairingCodeSheet,
  PairedDesktops,
  PairDesktopButton,
  UnpairedHome,
  OnboardingChoice,
  NotificationChoices,
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
  HomeSettingsSectionTitle,
  HomeSettingsSpacer,
  ChatTextPreview,
  OptionSheet,
};

export { TamaguiThemeProvider } from './adapters/theme/TamaguiThemeProvider';
