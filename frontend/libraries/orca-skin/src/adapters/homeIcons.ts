import AddIcon from 'lucide-react-native/dist/esm/icons/plus.mjs';
import type { LucideIcon } from 'lucide-react-native';
import AboutIcon from 'lucide-react-native/dist/esm/icons/info.mjs';
import BotIcon from 'lucide-react-native/dist/esm/icons/bot.mjs';
import SettingsIcon from 'lucide-react-native/dist/esm/icons/settings.mjs';
import AgentsIcon from 'lucide-react-native/dist/esm/icons/square-terminal.mjs';
import AppearanceIcon from 'lucide-react-native/dist/esm/icons/sun-moon.mjs';
import AutomationsIcon from 'lucide-react-native/dist/esm/icons/calendar-clock.mjs';
import BackIcon from 'lucide-react-native/dist/esm/icons/chevron-left.mjs';
import BellIcon from 'lucide-react-native/dist/esm/icons/bell.mjs';
import BrowserIcon from 'lucide-react-native/dist/esm/icons/globe.mjs';
import ChatUiIcon from 'lucide-react-native/dist/esm/icons/message-square.mjs';
import CheckIcon from 'lucide-react-native/dist/esm/icons/check.mjs';
import ChevronRightIcon from 'lucide-react-native/dist/esm/icons/chevron-right.mjs';
import ClearIcon from 'lucide-react-native/dist/esm/icons/x.mjs';
import OptionsIcon from 'lucide-react-native/dist/esm/icons/sliders-horizontal.mjs';
import PrivacyPolicyIcon from 'lucide-react-native/dist/esm/icons/shield.mjs';
import SearchIcon from 'lucide-react-native/dist/esm/icons/search.mjs';
import SupportIcon from 'lucide-react-native/dist/esm/icons/life-buoy.mjs';
import TerminalIcon from 'lucide-react-native/dist/esm/icons/terminal.mjs';
import TroubleshootingIcon from 'lucide-react-native/dist/esm/icons/wrench.mjs';
import UsageIcon from 'lucide-react-native/dist/esm/icons/gauge.mjs';
import VoiceIcon from 'lucide-react-native/dist/esm/icons/mic.mjs';
import type { HomeIconKind } from '@lib/skin';

/**
 * The Orca home's glyphs.
 *
 * Each icon is imported one file at a time: the barrel pulls 1,713 modules into
 * the bundle, these cost a fraction of that.
 */
export const homeIcons: Record<HomeIconKind, LucideIcon> = {
  search: SearchIcon,
  options: OptionsIcon,
  add: AddIcon,
  back: BackIcon,
  bots: BotIcon,
  settings: SettingsIcon,
  agents: AgentsIcon,
  usage: UsageIcon,
  automations: AutomationsIcon,
  appearance: AppearanceIcon,
  terminal: TerminalIcon,
  chatUi: ChatUiIcon,
  browser: BrowserIcon,
  voice: VoiceIcon,
  notifications: BellIcon,
  troubleshooting: TroubleshootingIcon,
  about: AboutIcon,
  privacyPolicy: PrivacyPolicyIcon,
  support: SupportIcon,
};

export { BackIcon, CheckIcon, ChevronRightIcon, ClearIcon, SearchIcon };
