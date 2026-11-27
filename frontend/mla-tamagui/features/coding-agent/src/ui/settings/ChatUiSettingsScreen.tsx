import { useCodingAgentPorts } from '../../state/ports';
import { useChatText, useSkin } from '@lib/skin';
import { useState } from 'react';
import { Platform } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { OrcaSettingsScreen } from './OrcaSettingsScreen';

const fonts = [
  {
    value: 'system',
    label: 'System',
    subtitle: Platform.OS === 'ios' ? 'SF Pro' : 'Roboto',
  },
  {
    value: 'serif',
    label: 'Serif',
    subtitle: Platform.OS === 'ios' ? 'New York' : 'Noto Serif',
  },
  { value: 'mono', label: 'Monospace', subtitle: 'Same as code' },
] as const;

const answer =
  'The test waits on `setTimeout(0)` while the stream is still flushing, so it passes only when the flush wins.';
const code =
  '\n\n```ts\nawait vi.waitFor(() =>\n  expect(rows).toHaveLength(3))\n```';

export function ChatUiSettingsScreen() {
  const { copyMessage } = useCodingAgentPorts();
  const { value, update, reset, stored } = useChatText();
  const {
    ChatSizeControl,
    ChatSpacingControl,
    ChatTextPreview,
    HomeSettingsGroup,
    HomeSettingsRow,
    HomeSettingsNote,
    HomeSettingsSectionTitle,
    HomeSettingsSpacer,
    OptionSheet,
  } = useSkin();
  const { bottom } = useSafeAreaInsets();
  const [picking, setPicking] = useState(false);
  return (
    <OrcaSettingsScreen title="Chat UI">
      <ChatTextPreview
        question="Why is the resume test flaky?"
        activity="Read 3 files · Ran pnpm test"
        answer={answer + code}
        copyCode={copyMessage}
      />
      <HomeSettingsSectionTitle>TEXT</HomeSettingsSectionTitle>
      <HomeSettingsGroup separatorInset={16}>
        <ChatSizeControl label="Text size" />
        <HomeSettingsRow
          label="Font"
          value={fonts.find((font) => font.value === value.font)?.label}
          navigates
          onPress={() => setPicking(true)}
        />
        <ChatSpacingControl label />
      </HomeSettingsGroup>
      <HomeSettingsNote>
        Pinch in a chat to change it; it saves here.
      </HomeSettingsNote>
      <HomeSettingsSectionTitle>DEFAULT VIEW</HomeSettingsSectionTitle>
      <HomeSettingsGroup>
        <HomeSettingsRow label="Open sessions in Chat UI" value="On" />
      </HomeSettingsGroup>
      <HomeSettingsNote>
        Sessions on this device open in Chat UI.
      </HomeSettingsNote>
      <HomeSettingsSpacer size="lg" />
      <HomeSettingsGroup>
        <HomeSettingsRow
          label="Reset text settings"
          disabled={!stored}
          onPress={reset}
        />
      </HomeSettingsGroup>
      {picking && (
        <OptionSheet
          title="Chat font"
          dismissLabel="Dismiss Chat font"
          bottomInset={bottom}
          options={fonts.map((font) => ({ id: font.value, label: font.label, subtitle: font.subtitle, selected: value.font === font.value }))}
          onSelect={(id) => update({ font: fonts.find((font) => font.value === id)?.value ?? 'system' })}
          onClose={() => setPicking(false)}
        />
      )}
    </OrcaSettingsScreen>
  );
}
