import { useAgentComponents } from '@lib/agent-ui-contracts';
import {
  ChatSizeControl,
  ChatSpacingControl,
  homePalettes,
  useChatText,
  useOrcaScheme,
} from '@lib/orca-design-system';
import { useState } from 'react';
import {
  Modal,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  View,
} from 'react-native';
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
  const { value, update, reset, stored } = useChatText();
  const {
    ChatMessageText,
    HomeSettingsGroup,
    HomeSettingsRow,
    HomeSettingsNote,
  } = useAgentComponents();
  const colors = homePalettes[useOrcaScheme()];
  const { bottom } = useSafeAreaInsets();
  const [picking, setPicking] = useState(false);
  const sectionTitle = [styles.heading, { color: colors.textSecondary }];
  return (
    <OrcaSettingsScreen title="Chat UI">
      <View style={[styles.preview, { borderColor: colors.borderSubtle }]}>
        <View
          style={[
            styles.bubble,
            {
              backgroundColor: colors.bgRaised,
              borderColor: colors.borderSubtle,
            },
          ]}
        >
          <ChatMessageText text="Why is the resume test flaky?" />
        </View>
        <Text
          style={{
            fontSize: value.size,
            color: colors.textSecondary,
          }}
        >
          Read 3 files · Ran pnpm test
        </Text>
        <ChatMessageText text={answer + code} />
      </View>
      <Text style={sectionTitle}>TEXT</Text>
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
      <Text style={sectionTitle}>DEFAULT VIEW</Text>
      <HomeSettingsGroup>
        <HomeSettingsRow label="Open sessions in Chat UI" value="On" />
      </HomeSettingsGroup>
      <HomeSettingsNote>
        Sessions on this device open in Chat UI.
      </HomeSettingsNote>
      <View style={{ marginTop: 24 }}>
        <HomeSettingsGroup>
          <HomeSettingsRow
            label="Reset text settings"
            disabled={!stored}
            onPress={reset}
          />
        </HomeSettingsGroup>
      </View>
      {picking && (
        <Modal
          visible
          transparent
          animationType="none"
          onRequestClose={() => setPicking(false)}
        >
          <View style={styles.overlay}>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel="Dismiss Chat font"
              onPress={() => setPicking(false)}
              style={StyleSheet.absoluteFill}
            />
            <View
              style={[
                styles.sheet,
                {
                  paddingBottom: bottom + 16,
                  backgroundColor: colors.bgRaised,
                },
              ]}
            >
              <View
                style={[styles.grabber, { backgroundColor: colors.textMuted }]}
              />
              <Text
                accessibilityRole="header"
                style={[styles.fontTitle, { color: colors.textSecondary }]}
              >
                Chat font
              </Text>
              <View
                style={{
                  borderRadius: 16,
                  backgroundColor: colors.bgGroup,
                  overflow: 'hidden',
                }}
              >
                {fonts.map((font, i) => (
                  <Pressable
                    key={font.value}
                    accessibilityRole="button"
                    accessibilityLabel={font.label}
                    accessibilityState={{ selected: value.font === font.value }}
                    onPress={() => {
                      update({ font: font.value });
                      setPicking(false);
                    }}
                    style={[
                      styles.fontRow,
                      {
                        borderTopWidth: i ? StyleSheet.hairlineWidth : 0,
                        borderColor: colors.borderSubtle,
                      },
                    ]}
                  >
                    <View style={{ gap: 3 }}>
                      <Text
                        style={{
                          fontSize: 15,
                          fontWeight: '600',
                          color: colors.textPrimary,
                        }}
                      >
                        {font.label}
                      </Text>
                      <Text
                        style={{ fontSize: 13, color: colors.textSecondary }}
                      >
                        {font.subtitle}
                      </Text>
                    </View>
                    {value.font === font.value && (
                      <Text style={{ fontSize: 19, color: colors.textPrimary }}>
                        ✓
                      </Text>
                    )}
                  </Pressable>
                ))}
              </View>
            </View>
          </View>
        </Modal>
      )}
    </OrcaSettingsScreen>
  );
}

const styles = StyleSheet.create({
  preview: {
    gap: 12,
    padding: 12,
    marginBottom: 8,
    borderWidth: 1,
    borderRadius: 16,
  },
  bubble: {
    alignSelf: 'flex-end',
    paddingVertical: 8,
    paddingHorizontal: 12,
    borderWidth: 1,
    borderRadius: 16,
  },
  heading: {
    paddingTop: 12,
    paddingBottom: 4,
    paddingHorizontal: 16,
    fontSize: 13,
    fontWeight: '500',
  },
  overlay: {
    flex: 1,
    justifyContent: 'flex-end',
    backgroundColor: 'rgba(0,0,0,0.55)',
  },
  sheet: {
    borderTopLeftRadius: 22,
    borderTopRightRadius: 22,
    paddingHorizontal: 12,
    paddingTop: 8,
  },
  grabber: { height: 5, width: 36, borderRadius: 3, alignSelf: 'center' },
  fontTitle: {
    paddingHorizontal: 16,
    paddingTop: 12,
    paddingBottom: 8,
    fontSize: 14,
  },
  fontRow: {
    minHeight: 60,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
  },
});
