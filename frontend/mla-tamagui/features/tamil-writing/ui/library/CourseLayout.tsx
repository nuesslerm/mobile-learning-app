import { CompactHeader, FloatingSurface, useCompactColors, useKeyboardFrame } from '@lib/tamagui-design-system';
import { ChevronLeft } from '@lib/tamagui-design-system/icons';
import { router, type Href } from 'expo-router';
import type { ReactNode } from 'react';
import { KeyboardAvoidingView, Platform, Pressable, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

export function backTo(href: Href) {
  if (router.canGoBack()) router.back();
  else router.replace(href);
}

export function CourseLayout({ title, children, testID, onBack, header, leading, trailing }: {
  title: string;
  children: ReactNode;
  testID: string;
  onBack: () => void;
  header?: ReactNode;
  leading?: ReactNode;
  trailing?: ReactNode;
}) {
  const colors = useCompactColors();
  const { top } = useSafeAreaInsets();
  const keyboard = useKeyboardFrame();
  return (
    <KeyboardAvoidingView behavior={Platform.OS === 'ios' ? 'padding' : undefined}
      style={[styles.page, {
        backgroundColor: colors.background,
        paddingTop: header ? 0 : top,
        paddingBottom: Platform.OS === 'web' ? keyboard.height : 0,
      }]} testID={testID}>
      {header ?? <CompactHeader onClose={onBack} closeIcon={ChevronLeft} title={title} leading={leading} trailing={trailing} />}
      {children}
    </KeyboardAvoidingView>
  );
}

export function CourseButton({ label, onPress, disabled, testID }: {
  label: string;
  onPress: () => void;
  disabled?: boolean;
  testID?: string;
}) {
  const colors = useCompactColors();
  return (
    <Pressable accessibilityRole="button" disabled={disabled} onPress={onPress} testID={testID}
      style={{ opacity: disabled ? 0.45 : 1 }}>
      <FloatingSurface style={styles.button}>
        <Text style={{ color: disabled ? colors.muted : colors.green, fontWeight: '700', fontSize: 16 }}>{label}</Text>
      </FloatingSurface>
    </Pressable>
  );
}

export function CourseMessage({ text, retry }: { text: string; retry?: () => void }) {
  const colors = useCompactColors();
  return (
    <View style={styles.message}>
      <Text style={{ color: colors.muted, textAlign: 'center' }}>{text}</Text>
      {retry && <CourseButton label="Try again" onPress={retry} />}
    </View>
  );
}

const styles = StyleSheet.create({
  page: { flex: 1 },
  button: { borderRadius: 24, minHeight: 48, justifyContent: 'center', alignItems: 'center', paddingHorizontal: 16 },
  message: { margin: 20, gap: 16 },
});
