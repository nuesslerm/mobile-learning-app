import { useChatText, useSkin, type ChatTextPreviewProps, type HomeSettingsSectionTitleProps, type HomeSettingsSpacerProps, type OptionSheetProps } from '@lib/skin';
import { Pressable, StyleSheet, Text, View } from 'react-native';

import { BottomSheet } from './BottomSheet';
import { useChatPalette } from './chatPalette';

export function HomeSettingsSectionTitle(props: HomeSettingsSectionTitleProps) {
  const colors = useChatPalette();
  return <Text style={[styles.heading, { color: colors.textSecondary }]}>{props.children}</Text>;
}

export function HomeSettingsSpacer(props: HomeSettingsSpacerProps) {
  return <View style={{ height: props.size === 'lg' ? 24 : 16 }} />;
}

export function ChatTextPreview(props: ChatTextPreviewProps) {
  const { ChatMessageText } = useSkin();
  const { value } = useChatText();
  const colors = useChatPalette();
  return (
    <View style={[styles.preview, { borderColor: colors.borderSubtle }]}>
      <View style={[styles.bubble, { backgroundColor: colors.bgRaised, borderColor: colors.userBorder }]}>
        <ChatMessageText tone="user" text={props.question} />
      </View>
      <Text style={{ fontSize: value.size, lineHeight: (20 * value.size) / 14, color: colors.textMuted }}>{props.activity}</Text>
      <ChatMessageText text={props.answer} copyCode={props.copyCode} />
    </View>
  );
}

export function OptionSheet(props: OptionSheetProps) {
  const colors = useChatPalette();
  return (
    <BottomSheet
      label={props.dismissLabel}
      onClose={props.onClose}
      style={[styles.sheet, { paddingBottom: props.bottomInset + 16, backgroundColor: colors.bgRaised }]}
    >
      {(dismiss) => (
        <>
          <View style={[styles.grabber, { backgroundColor: colors.textMuted }]} />
          <Text accessibilityRole="header" style={[styles.title, { color: colors.textSecondary }]}>
            {props.title}
          </Text>
          <View style={{ borderRadius: 16, backgroundColor: colors.bgGroup, overflow: 'hidden' }}>
            {props.options.map((option, i) => (
              <Pressable
                key={option.id}
                accessibilityRole="button"
                accessibilityLabel={option.label}
                accessibilityState={{ selected: Boolean(option.selected) }}
                onPress={() => {
                  props.onSelect(option.id);
                  dismiss();
                }}
                style={[styles.row, { borderTopWidth: i ? StyleSheet.hairlineWidth : 0, borderColor: colors.borderSubtle }]}
              >
                <View style={{ gap: 3 }}>
                  <Text style={{ fontSize: 15, fontWeight: '600', color: colors.textPrimary }}>{option.label}</Text>
                  {option.subtitle ? <Text style={{ fontSize: 13, color: colors.textSecondary }}>{option.subtitle}</Text> : null}
                </View>
                {option.selected && <Text style={{ fontSize: 19, color: colors.textPrimary }}>✓</Text>}
              </Pressable>
            ))}
          </View>
        </>
      )}
    </BottomSheet>
  );
}

const styles = StyleSheet.create({
  preview: { gap: 12, padding: 12, marginBottom: 8, borderWidth: 1, borderRadius: 16 },
  bubble: { alignSelf: 'flex-end', paddingVertical: 8, paddingHorizontal: 12, borderWidth: 1, borderRadius: 16 },
  heading: { paddingTop: 12, paddingBottom: 4, paddingHorizontal: 16, fontSize: 13, fontWeight: '500' },
  sheet: { borderTopLeftRadius: 22, borderTopRightRadius: 22, paddingHorizontal: 12, paddingTop: 8 },
  grabber: { height: 5, width: 36, borderRadius: 3, alignSelf: 'center' },
  title: { paddingHorizontal: 16, paddingTop: 12, paddingBottom: 8, fontSize: 14 },
  row: { minHeight: 60, flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingHorizontal: 16 },
});
