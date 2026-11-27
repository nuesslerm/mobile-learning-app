import {
  useComposerEditing,
  useSkin,
  type ComposerProps,
  type ComposerSendButtonProps,
} from '@lib/skin';
import { ArrowUp } from 'lucide-react-native';
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  type TextInputInstance,
  View,
} from 'react-native';
import { useChatPalette } from './chatPalette';

export function ComposerSendButton({
  label,
  canSend,
  sending = false,
  onPress,
  onLongPress,
}: ComposerSendButtonProps) {
  const colors = useChatPalette();
  const enabled = canSend && !sending;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled: !enabled, busy: sending }}
      aria-busy={sending}
      disabled={!enabled}
      onPress={onPress}
      onLongPress={onLongPress}
      hitSlop={3}
      style={({ pressed }) => [
        styles.send,
        { backgroundColor: canSend ? colors.surfaceBright : colors.bgRaised },
        pressed && enabled && styles.pressed,
      ]}
    >
      {sending ? (
        <ActivityIndicator size="small" color={colors.bgBase} />
      ) : (
        <ArrowUp
          size={19}
          color={canSend ? colors.bgBase : colors.textMuted}
          strokeWidth={2.4}
        />
      )}
    </Pressable>
  );
}

export function Composer({
  suggestions,
  notice,
  onPickSuggestion,
  value,
  placeholder,
  sendLabel,
  canSend,
  sending,
  leadingControls,
  trailingControls,
  onChangeText,
  onSend,
}: ComposerProps) {
  const colors = useChatPalette();
  const {
    ComposerSendButton: Send,
    ComposerAddButton,
    ComposerSuggestionList,
  } = useSkin();
  const { field, selection, trigger, kind, selectionChanged, textChanged, open, pick, addDismissed } =
    useComposerEditing<TextInputInstance>({ value, notice, onChangeText, onPickSuggestion });
  return (
    <>
      {!!notice && (
        <View style={styles.notice} accessibilityRole="alert" accessibilityLiveRegion="assertive">
          <Text style={[styles.noticeText, { color: colors.statusRed }]}>{notice}</Text>
        </View>
      )}
      {suggestions && trigger && (
        <ComposerSuggestionList
          source={suggestions}
          sigil={trigger.sigil}
          query={trigger.query}
          kind={kind}
          onPick={pick}
        />
      )}
      <View style={styles.inset}>
        <View
          style={[
            styles.box,
            {
              backgroundColor: colors.composerSurface,
              borderColor: colors.composerBorder,
            },
          ]}
        >
          <View style={styles.frame}>
            <TextInput
              ref={field}
              selection={selection}
              onSelectionChange={({ nativeEvent }) => selectionChanged(nativeEvent.selection)}
              value={value}
              onChangeText={textChanged}
              placeholder={placeholder}
              placeholderTextColor={colors.textMuted}
              selectionColor={colors.accentBlue}
              multiline
              textAlignVertical="top"
              style={[styles.input, { color: colors.textPrimary }]}
            />
          </View>
          <View style={styles.actions}>
            <View style={styles.leading}>
              {suggestions && (
                <ComposerAddButton
                  onSelect={open}
                  onDismiss={addDismissed}
                />
              )}
              {leadingControls}
            </View>
            {trailingControls}
            <Send
              label={sendLabel}
              canSend={canSend}
              sending={sending}
              onPress={onSend}
            />
          </View>
        </View>
      </View>
    </>
  );
}

const styles = StyleSheet.create({
  notice: { alignItems: 'center', paddingHorizontal: 12, paddingBottom: 4 },
  noticeText: { flexShrink: 1, fontSize: 12, fontWeight: '600' },
  inset: { paddingHorizontal: 12, paddingTop: 6, paddingBottom: 8 },
  box: { borderWidth: 1, borderRadius: 24, padding: 12 },
  frame: { width: '100%', overflow: 'hidden' },
  input: {
    width: '100%',
    minHeight: 30,
    maxHeight: 154,
    fontSize: 16,
    lineHeight: 22,
    paddingHorizontal: 6,
    paddingTop: 2,
    paddingBottom: 2,
  },
  actions: { marginTop: 8, flexDirection: 'row', alignItems: 'center', gap: 8 },
  leading: {
    flex: 1,
    minWidth: 0,
    flexDirection: 'row',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: 8,
  },
  send: {
    width: 38,
    height: 38,
    borderRadius: 999,
    alignItems: 'center',
    justifyContent: 'center',
  },
  pressed: { opacity: 0.7 },
});
