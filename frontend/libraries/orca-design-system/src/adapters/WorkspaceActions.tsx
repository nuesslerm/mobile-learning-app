import type { WorkspaceActionsProps } from '@lib/agent-ui-contracts';
import { useEffect, useRef, useState } from 'react';
import {
  Keyboard,
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  type TextInputInstance,
  View,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import Pencil from 'lucide-react-native/dist/esm/icons/pencil.mjs';
import Pin from 'lucide-react-native/dist/esm/icons/pin.mjs';
import PinOff from 'lucide-react-native/dist/esm/icons/pin-off.mjs';
import { useChatPalette } from './chatPalette';
import { useReducedMotionEnabled } from './useReducedMotion';

export function WorkspaceActions(props: WorkspaceActionsProps) {
  const colors = useChatPalette();
  const { bottom } = useSafeAreaInsets();
  const reduced = useReducedMotionEnabled();
  const [renaming, setRenaming] = useState(false);
  const input = useRef<TextInputInstance>(null);
  const backdropPressed = useRef(false);
  const [focused, setFocused] = useState(false);
  const [keyboardShowing, setKeyboardShowing] = useState(false);
  useEffect(() => {
    if (Platform.OS !== 'ios') return;
    const show = Keyboard.addListener('keyboardWillShow', () =>
      setKeyboardShowing(true)
    );
    const hide = Keyboard.addListener('keyboardWillHide', () =>
      setKeyboardShowing(false)
    );
    return () => {
      show.remove();
      hide.remove();
    };
  }, []);
  const [draft, setDraft] = useState(props.name);
  const ink = { color: colors.textPrimary };
  const muted = { color: colors.textSecondary };
  const PinIcon = props.pinned ? PinOff : Pin;
  const save = () => {
    if (!props.saving) props.onRename(draft.trim());
  };
  return (
    <Modal
      visible
      transparent
      animationType={reduced ? 'fade' : 'slide'}
      onRequestClose={props.onClose}
    >
      <KeyboardAvoidingView
        behavior={Platform.OS === 'ios' ? 'padding' : undefined}
        style={styles.overlay}
      >
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Dismiss"
          onPointerDown={() => {
            backdropPressed.current = true;
          }}
          onPointerCancel={() => {
            backdropPressed.current = false;
          }}
          onPress={(event) => {
            // Browsers can send the row’s trailing click to the newly opened backdrop.
            const keyboardClick =
              'detail' in event.nativeEvent && event.nativeEvent.detail === 0;
            if (
              Platform.OS !== 'web' ||
              backdropPressed.current ||
              keyboardClick
            )
              props.onClose();
            backdropPressed.current = false;
          }}
          style={StyleSheet.absoluteFill}
        />
        <View
          accessibilityViewIsModal
          style={[
            styles.sheet,
            {
              backgroundColor: colors.sheetSurface,
              paddingBottom: (keyboardShowing ? 0 : bottom) + 16,
            },
          ]}
        >
          <View
            style={[styles.handle, { backgroundColor: colors.textMuted }]}
          />
          {renaming ? (
            <>
              <Text accessibilityRole="header" style={[styles.title, ink]}>
                Rename Workspace
              </Text>
              {!!props.branch && (
                <Text style={[styles.hint, muted]}>
                  Leave empty to use the branch name.
                </Text>
              )}
              <TextInput
                accessibilityLabel="Workspace name"
                ref={input}
                defaultValue={props.name}
                onFocus={() => {
                  setFocused(true);
                  if (Platform.OS === 'ios')
                    requestAnimationFrame(() =>
                      input.current?.setSelection(0, draft.length)
                    );
                }}
                onBlur={() => setFocused(false)}
                onChangeText={setDraft}
                placeholder={props.automaticName}
                placeholderTextColor={colors.textMuted}
                autoFocus
                selectTextOnFocus
                autoCapitalize="none"
                autoCorrect={false}
                spellCheck={false}
                returnKeyType="done"
                onSubmitEditing={save}
                style={[
                  styles.input,
                  ink,
                  {
                    borderColor: focused
                      ? colors.accentBlue
                      : colors.borderSubtle,
                    borderWidth: focused ? 2 : 1,
                    backgroundColor: colors.bgGroup,
                  },
                ]}
              />
              <View style={styles.buttons}>
                <Pressable
                  disabled={props.saving}
                  accessibilityRole="button"
                  onPress={props.onClose}
                  style={styles.button}
                >
                  <Text style={muted}>Cancel</Text>
                </Pressable>
                <Pressable
                  disabled={props.saving}
                  accessibilityRole="button"
                  onPress={save}
                  style={[
                    styles.button,
                    { backgroundColor: colors.textPrimary },
                  ]}
                >
                  <Text style={{ color: colors.bgBase }}>Save</Text>
                </Pressable>
              </View>
            </>
          ) : (
            <>
              <Text accessibilityRole="header" style={[styles.heading, ink]}>
                {props.name}
              </Text>
              {!!props.branch && (
                <Text style={[styles.hint, muted]}>{props.branch}</Text>
              )}
              <View style={[styles.rows, { backgroundColor: colors.bgGroup }]}>
                <Pressable
                  accessibilityRole="button"
                  disabled={props.saving}
                  style={styles.row}
                  onPress={() => setRenaming(true)}
                >
                  <Pencil size={16} color={colors.textSecondary} />
                  <Text style={[styles.rowLabel, ink]}>Rename</Text>
                </Pressable>
                <View
                  style={[
                    styles.rule,
                    { backgroundColor: colors.borderSubtle },
                  ]}
                />
                <Pressable
                  accessibilityRole="button"
                  disabled={props.saving}
                  style={styles.row}
                  onPress={props.onPin}
                >
                  <PinIcon size={16} color={colors.textSecondary} />
                  <Text style={[styles.rowLabel, ink]}>
                    {props.pinned ? 'Unpin' : 'Pin'}
                  </Text>
                </Pressable>
              </View>
            </>
          )}
          {!!props.error && (
            <Text accessibilityRole="alert" style={[styles.hint, ink]}>
              {props.error}
            </Text>
          )}
        </View>
      </KeyboardAvoidingView>
    </Modal>
  );
}
const styles = StyleSheet.create({
  overlay: {
    flex: 1,
    justifyContent: 'flex-end',
    backgroundColor: 'rgba(0,0,0,0.55)',
  },
  sheet: {
    borderTopLeftRadius: 24,
    borderTopRightRadius: 24,
    paddingHorizontal: 12,
  },
  handle: {
    alignSelf: 'center',
    width: 36,
    height: 4,
    borderRadius: 2,
    marginTop: 10,
    marginBottom: 12,
    opacity: 0.45,
  },
  title: { fontSize: 15, fontWeight: '600', paddingHorizontal: 4 },
  heading: { fontSize: 16, fontWeight: '600', paddingHorizontal: 12 },
  hint: { fontSize: 13, marginTop: 3, paddingHorizontal: 4 },
  input: {
    height: 44,
    marginTop: 10,
    borderWidth: 1,
    borderRadius: 12,
    paddingHorizontal: 12,
    fontSize: 16,
  },
  rows: { borderRadius: 14, marginTop: 10 },
  row: {
    flexDirection: 'row',
    gap: 12,
    paddingHorizontal: 14,
    minHeight: 45,
    alignItems: 'center',
  },
  rowLabel: { fontSize: 15 },
  rule: { height: StyleSheet.hairlineWidth, marginLeft: 42 },
  buttons: {
    flexDirection: 'row',
    justifyContent: 'flex-end',
    gap: 8,
    marginTop: 12,
  },
  button: { borderRadius: 12, paddingHorizontal: 16, paddingVertical: 9 },
});
