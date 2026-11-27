import { useCodingAgentPorts } from '../../state/ports';
import {
  useSkin,
  useSkinTheme,
  useChatText,
} from '@lib/skin';
import { useState } from 'react';
import { Platform, Pressable, StyleSheet, Text, View } from 'react-native';
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
    BottomSheet,
    ChatSizeControl,
    ChatSpacingControl,
    ChatMessageText,
    HomeSettingsGroup,
    HomeSettingsRow,
    HomeSettingsNote,
  } = useSkin();
  const { colors } = useSkinTheme();
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
              borderColor: colors.userBorder,
            },
          ]}
        >
          <ChatMessageText tone="user" text="Why is the resume test flaky?" />
        </View>
        <Text
          style={{
            fontSize: value.size,
            lineHeight: (20 * value.size) / 14,
            color: colors.textMuted,
          }}
        >
          Read 3 files · Ran pnpm test
        </Text>
        <ChatMessageText text={answer + code} copyCode={copyMessage} />
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
        <BottomSheet
          label="Dismiss Chat font"
          onClose={() => setPicking(false)}
          style={[
            styles.sheet,
            { paddingBottom: bottom + 16, backgroundColor: colors.bgRaised },
          ]}
        >
          {(dismiss) => (
            <>
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
                      dismiss();
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
            </>
          )}
        </BottomSheet>
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
