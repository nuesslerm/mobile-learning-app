import { BottomSheet } from './BottomSheet';
import { useState } from 'react';
import {
  Keyboard,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import type { PairingCodeSheetProps } from '@lib/agent-ui-contracts';
import { homePalettes, useOrcaScheme } from './appearance';

export function PairingCodeSheet({ visible, ...props }: PairingCodeSheetProps) {
  return visible ? <CodeEntry {...props} /> : null;
}

function CodeEntry({
  onCancel,
  onSubmit,
  bottomInset,
  keyboardInset,
}: Omit<PairingCodeSheetProps, 'visible'>) {
  const scheme = useOrcaScheme();
  const colors = homePalettes[scheme];
  const [text, setText] = useState('');
  const [focused, setFocused] = useState(false);
  const [hasKeyboard, setHasKeyboard] = useState(false);
  const surface = scheme === 'light' ? '#f5f5f5' : '#171717';
  const cancel = () => {
    Keyboard.dismiss();
    onCancel();
  };
  const valid = !!text.trim();
  const save = () => {
    if (valid) {
      Keyboard.dismiss();
      onSubmit(text.trim());
    }
  };
  return (
    <BottomSheet
      avoidKeyboard
      keyboardInset={Platform.OS === 'web' ? keyboardInset : undefined}
      onClose={cancel}
      style={[
        sheet.panel,
        {
          backgroundColor: surface,
          paddingBottom: Math.max(hasKeyboard ? 0 : bottomInset, 16),
        },
      ]}
    >
      {(dismiss) => (
        <>
          <View
            pointerEvents="none"
            style={{
              position: 'absolute',
              left: 0,
              right: 0,
              bottom: -500,
              height: 500,
              backgroundColor: surface,
            }}
          />
          <View style={[sheet.handle, { backgroundColor: colors.textMuted }]} />
          <View style={sheet.header}>
            <Text style={[sheet.title, { color: colors.textPrimary }]}>
              Paste pairing code
            </Text>
            <Text style={[sheet.message, { color: colors.textMuted }]}>
              Copy the code shown under the QR on your computer.
            </Text>
          </View>
          <View
            style={[
              sheet.field,
              {
                backgroundColor: scheme === 'light' ? colors.bgBase : '#1f1f1f',
                borderColor: focused ? colors.accentBlue : '#737373',
              },
              focused && {
                outlineColor: colors.accentBlue,
                outlineStyle: 'solid',
                outlineWidth: 1,
              },
            ]}
          >
            <TextInput
              autoFocus
              accessibilityLabel="Pairing code"
              testID="MobilePairCode"
              value={text}
              onChangeText={setText}
              autoCapitalize="none"
              autoCorrect={false}
              returnKeyType="done"
              onSubmitEditing={() => {
                if (valid) dismiss(save);
              }}
              onFocus={() => {
                setFocused(true);
                setHasKeyboard(true);
              }}
              onBlur={() => {
                setFocused(false);
                setHasKeyboard(false);
              }}
              placeholder="orca://pair?code=... or paste the code"
              placeholderTextColor={colors.textMuted}
              selectionColor={colors.accentBlue}
              style={[sheet.fieldText, { color: colors.textPrimary }]}
            />
          </View>
          <View style={sheet.actions}>
            <Pressable
              accessibilityRole="button"
              onPress={() => dismiss()}
              style={sheet.button}
            >
              <Text
                style={{
                  color: colors.textSecondary,
                  fontSize: 14,
                  fontWeight: '500',
                }}
              >
                Cancel
              </Text>
            </Pressable>
            <Pressable
              accessibilityRole="button"
              disabled={!valid}
              onPress={() => dismiss(save)}
              style={[
                sheet.button,
                {
                  opacity: valid ? 1 : 0.4,
                  backgroundColor: colors.textPrimary,
                },
              ]}
            >
              <Text
                style={{
                  color: colors.bgBase,
                  fontSize: 14,
                  fontWeight: '600',
                }}
              >
                Save
              </Text>
            </Pressable>
          </View>
        </>
      )}
    </BottomSheet>
  );
}

const sheet = StyleSheet.create({
  panel: {
    borderCurve: 'continuous',
    ...Platform.select({
      ios: {
        shadowColor: '#000',
        shadowOffset: { width: 0, height: -2 },
        shadowOpacity: 0.2,
        shadowRadius: 10,
      },
      android: { elevation: 8 },
    }),
    borderTopLeftRadius: 24,
    borderTopRightRadius: 24,
    paddingHorizontal: 12,
    paddingTop: 8,
  },
  handle: {
    alignSelf: 'center',
    width: 36,
    height: 5,
    borderRadius: 999,
    opacity: 0.5,
    marginBottom: 12,
  },
  header: { paddingHorizontal: 4, paddingBottom: 8 },
  title: { fontSize: 15, fontWeight: '600' },
  message: { fontSize: 13, marginTop: 2 },
  field: {
    minHeight: 44,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 12,
    borderWidth: 1,
    borderRadius: 12,
  },
  fieldText: {
    outlineWidth: 0,
    flex: 1,
    minWidth: 0,
    alignSelf: 'stretch',
    padding: 0,
    margin: 0,
    fontSize: 16,
    includeFontPadding: false,
    textAlignVertical: 'center',
  },
  actions: {
    flexDirection: 'row',
    justifyContent: 'flex-end',
    gap: 8,
    marginTop: 12,
  },
  button: {
    borderCurve: 'continuous',
    borderRadius: 12,
    paddingHorizontal: 16,
    paddingVertical: 8,
  },
});
