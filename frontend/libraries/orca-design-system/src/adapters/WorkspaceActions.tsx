import type { WorkspaceActionsProps } from '@lib/agent-ui-contracts';
import { type ComponentRef, useEffect, useRef, useState } from 'react';
import {
  ActivityIndicator,
  ScrollView,
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
import Check from 'lucide-react-native/dist/esm/icons/check.mjs';
import Circle from 'lucide-react-native/dist/esm/icons/circle.mjs';
import Columns3 from 'lucide-react-native/dist/esm/icons/columns-3.mjs';
import Bell from 'lucide-react-native/dist/esm/icons/bell.mjs';
import BellOff from 'lucide-react-native/dist/esm/icons/bell-off.mjs';
import GitBranch from 'lucide-react-native/dist/esm/icons/git-branch.mjs';
import Pencil from 'lucide-react-native/dist/esm/icons/pencil.mjs';
import Pin from 'lucide-react-native/dist/esm/icons/pin.mjs';
import PinOff from 'lucide-react-native/dist/esm/icons/pin-off.mjs';
import { useChatPalette } from './chatPalette';
import { useReducedMotionEnabled } from './useReducedMotion';

export function WorkspaceActions(props: WorkspaceActionsProps) {
  const colors = useChatPalette();
  const ReadIcon = props.unread ? BellOff : Bell;
  const { bottom } = useSafeAreaInsets();
  const reduced = useReducedMotionEnabled();
  const [renaming, setRenaming] = useState(false);
  const input = useRef<TextInputInstance>(null);
  const backdropPressed = useRef(false);
  const [sheet, setSheet] = useState<ComponentRef<typeof View> | null>(null);
  useEffect(() => {
    if (Platform.OS !== 'web' || !(sheet instanceof HTMLElement))
      return;
    const element = sheet;
    let pressed: Element | null = null;
    const control = (event: Event) =>
      event.target instanceof Element
        ? event.target.closest('[role="button"], [role="radio"], button, input')
        : null;
    const down = (event: PointerEvent) => {
      pressed = control(event);
    };
    const cancel = () => {
      pressed = null;
    };
    const click = (event: MouseEvent) => {
      // A touch held on a checkout can release over a newly mounted menu action.
      if (event.detail > 0 && (!pressed || pressed !== control(event))) {
        event.preventDefault();
        event.stopPropagation();
      }
      pressed = null;
    };
    element.addEventListener('pointerdown', down, true);
    element.addEventListener('pointercancel', cancel, true);
    element.addEventListener('click', click, true);
    return () => {
      element.removeEventListener('pointerdown', down, true);
      element.removeEventListener('pointercancel', cancel, true);
      element.removeEventListener('click', click, true);
    };
  }, [sheet]);
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
          ref={Platform.OS === 'web' ? setSheet : undefined}
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
            style={[
              styles.handle,
              props.statusPicker && styles.statusHandle,
              { backgroundColor: colors.textMuted },
            ]}
          />
          {props.statusPicker ? (
            <>
              <Text style={[styles.statusTitle, muted]}>Move to Status</Text>
              <ScrollView
                bounces={false}
                style={[styles.statusList, { backgroundColor: colors.bgGroup }]}
              >
                {props.statusPicker.failed ? (
                  <Pressable
                    accessibilityRole="button"
                    onPress={props.onOpenStatuses}
                    style={styles.row}
                  >
                    <Text style={ink}>Could not load statuses. Retry</Text>
                  </Pressable>
                ) : !props.statusPicker.choices ? (
                  <ActivityIndicator
                    accessibilityLabel="Loading statuses"
                    style={styles.row}
                    color={colors.textSecondary}
                  />
                ) : (
                  props.statusPicker.choices.map((status, index) => (
                    <View key={status.id}>
                      {index > 0 && (
                        <View
                          style={[
                            styles.statusRule,
                            { backgroundColor: colors.borderSubtle },
                          ]}
                        />
                      )}
                      <Pressable
                        accessibilityRole="radio"
                        accessibilityLabel={status.label}
                        aria-checked={
                          props.statusPicker?.selected === status.id
                        }
                        accessibilityState={{
                          checked: props.statusPicker?.selected === status.id,
                          disabled: props.saving,
                        }}
                        onPress={() => props.onStatus(status.id)}
                        disabled={props.saving}
                        style={[styles.row, styles.statusRow]}
                      >
                        <Circle size={16} color={colors.textSecondary} />
                        <Text
                          style={[styles.rowLabel, styles.statusLabel, ink]}
                        >
                          {status.label}
                        </Text>
                        {props.statusPicker?.selected === status.id && (
                          <Check size={18} color={colors.textPrimary} />
                        )}
                      </Pressable>
                    </View>
                  ))
                )}
              </ScrollView>
            </>
          ) : renaming ? (
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
                  onPress={props.onSourceControl}
                >
                  <GitBranch size={16} color={colors.textSecondary} />
                  <Text style={[styles.rowLabel, ink]}>Source Control</Text>
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
                  onPress={props.onToggleRead}
                >
                  <ReadIcon size={16} color={colors.textSecondary} />
                  <Text style={[styles.rowLabel, ink]}>
                    {props.unread ? 'Mark Read' : 'Mark Unread'}
                  </Text>
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
                <View
                  style={[
                    styles.rule,
                    { backgroundColor: colors.borderSubtle },
                  ]}
                />
                <Pressable
                  accessibilityRole="button"
                  onPress={props.onOpenStatuses}
                  disabled={props.saving}
                  style={styles.row}
                >
                  <Columns3 size={16} color={colors.textSecondary} />
                  <Text style={[styles.rowLabel, ink]}>Move to Status</Text>
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
  statusHandle: { height: 5, marginTop: 8, opacity: 0.5 },
  statusTitle: {
    fontSize: 13,
    fontWeight: '500',
    marginHorizontal: 16,
    marginBottom: 9,
  },
  statusRow: { paddingHorizontal: 20, gap: 16, minHeight: 48 },
  statusRule: { height: StyleSheet.hairlineWidth, marginLeft: 52 },
  statusLabel: { flex: 1 },
  statusList: { maxHeight: '80%', flexGrow: 0, borderRadius: 16 },
  rule: { height: StyleSheet.hairlineWidth, marginLeft: 42 },
  buttons: {
    flexDirection: 'row',
    justifyContent: 'flex-end',
    gap: 8,
    marginTop: 12,
  },
  button: { borderRadius: 12, paddingHorizontal: 16, paddingVertical: 9 },
});
