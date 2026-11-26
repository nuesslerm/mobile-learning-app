import { useRef, useState } from 'react';
import { fileToken, suggestionTrigger } from './ComposerSuggestions';
import {
  useAgentComponents,
  type ComposerProps,
  type ComposerSuggestion,
  type ComposerSendButtonProps,
} from '@lib/agent-ui-contracts';
import { ArrowUp } from 'lucide-react-native';
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  TextInput,
  type TextInputInstance,
  Platform,
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
  } = useAgentComponents();
  const field = useRef<TextInputInstance>(null);
  const refocus = useRef(false);
  const [caret, setCaret] = useState(value.length);
  const [selection, setSelection] = useState<{ start: number; end: number }>();
  const [kind, setKind] = useState<ComposerSuggestion['kind'] | null>(null);
  const end = Math.min(caret, value.length);
  const trigger = suggestionTrigger(value, end);
  function insert(text: string, position: number) {
    onChangeText(text);
    setCaret(position);
    setSelection({ start: position, end: position });
  }
  function open(kind: ComposerSuggestion['kind']) {
    setKind(kind);
    if (kind === 'file') {
      const before = value.slice(0, end);
      const lead = before && !/\s$/.test(before) ? ' @' : '@';
      insert(before + lead + value.slice(end), end + lead.length);
    } else {
      insert(value.trimStart() ? '/ ' + value.trimStart() : '/', 1);
    }
    if (Platform.OS === 'ios') refocus.current = true;
    else field.current?.focus();
  }
  function pick(row: ComposerSuggestion) {
    if (!trigger) return;
    const token = row.kind === 'file' ? fileToken(row.token) : row.token;
    const before = value.slice(0, trigger.start) + token + ' ';
    insert(before + value.slice(end).replace(/^ /, ''), before.length);
    setKind(null);
    field.current?.focus();
  }
  return (
    <>
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
              onSelectionChange={({ nativeEvent }) => {
                setCaret(nativeEvent.selection.end);
                setSelection(undefined);
              }}
              value={value}
              onChangeText={(text) => {
                if (!text.startsWith('/')) setKind(null);
                onChangeText(text);
              }}
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
                  onDismiss={() => {
                    if (refocus.current) field.current?.focus();
                    refocus.current = false;
                  }}
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
