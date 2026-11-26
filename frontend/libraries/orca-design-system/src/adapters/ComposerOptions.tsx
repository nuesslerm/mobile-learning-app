import type { ComposerOptionsProps } from '@lib/agent-ui-contracts';
import { Check, ChevronLeft, ChevronRight } from 'lucide-react-native';
import { useEffect, useRef, useState } from 'react';
import {
  Keyboard,
  Modal,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { useChatPalette } from './chatPalette';

type Choice = { slug: string; name: string };

export function ComposerOptions(props: ComposerOptionsProps) {
  const colors = useChatPalette();
  const { bottom } = useSafeAreaInsets();
  const [page, setPage] = useState<'Model' | 'Permissions' | null>(null);
  const [models, setModels] = useState<readonly Choice[]>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const operation = useRef(0);
  useEffect(
    () => () => {
      operation.current += 1;
    },
    []
  );
  const ink = { color: colors.textPrimary };
  function close() {
    operation.current += 1;
    setBusy(false);
    setPage(null);
  }
  function open() {
    Keyboard.dismiss();
    setPage('Model');
    setError('');
    setModels(undefined);
    const request = ++operation.current;
    void props.loadModels().then(
      (choices) => {
        if (request === operation.current) setModels(choices);
      },
      () => {
        if (request === operation.current) setError('Could not load models.');
      }
    );
  }
  function select(slug: string) {
    if (busy) return;
    setError('');
    setBusy(true);
    const request = ++operation.current;
    const save = page === 'Model' ? props.setModel : props.setPermission;
    void save(slug).then(
      () => {
        if (request !== operation.current) return;
        setBusy(false);
        setPage('Model');
      },
      () => {
        if (request !== operation.current) return;
        setError('Could not save this choice.');
        setBusy(false);
      }
    );
  }
  const permission = props.permissions.find(
    (item) => item.slug === props.permission
  );
  const modelName = models?.find(
    (item) => item.slug === props.selectedModel
  )?.name;
  const label =
    props.selectedModel && props.selectedModel !== 'default'
      ? (modelName ?? props.selectedModel)
      : 'Model';
  const choices = page === 'Permissions' ? props.permissions : models;
  const selected =
    page === 'Permissions' ? props.permission : props.selectedModel;
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={'Model, ' + label}
        onPress={open}
        style={[styles.pill, { backgroundColor: colors.bgRaised }]}
      >
        <Text numberOfLines={1} style={[styles.title, ink]}>
          {label}
        </Text>
      </Pressable>
      {page !== null && (
        <Modal visible transparent animationType="slide" onRequestClose={close}>
          <View style={styles.overlay}>
            <Pressable
              accessibilityLabel="Dismiss"
              accessibilityRole="button"
              onPress={close}
              style={StyleSheet.absoluteFill}
            />
            <View
              accessibilityViewIsModal
              style={[
                styles.sheet,
                {
                  paddingBottom: Math.max(20, bottom + 12),
                  backgroundColor: colors.bgRaised,
                },
              ]}
            >
              <View
                style={[styles.handle, { backgroundColor: colors.textMuted }]}
              />
              <View style={styles.header}>
                {page === 'Permissions' && (
                  <Pressable
                    accessibilityRole="button"
                    accessibilityLabel="Back to models"
                    style={styles.back}
                    onPress={() => setPage('Model')}
                  >
                    <ChevronLeft size={20} color={colors.textPrimary} />
                  </Pressable>
                )}
                <Text style={[styles.heading, ink]}>{page}</Text>
                <Pressable
                  accessibilityRole="button"
                  style={styles.done}
                  onPress={close}
                >
                  <Text style={[styles.heading, ink]}>Done</Text>
                </Pressable>
              </View>
              <ScrollView bounces={false} keyboardShouldPersistTaps="handled">
                <Text style={[styles.section, { color: colors.textSecondary }]}>
                  {page}
                </Text>
                <View
                  style={[styles.group, { backgroundColor: colors.bgGroup }]}
                >
                  {choices?.map((choice, index) => (
                    <Pressable
                      key={choice.slug}
                      accessibilityRole="button"
                      accessibilityState={{
                        selected: selected === choice.slug,
                        disabled: busy,
                      }}
                      disabled={busy}
                      onPress={() => select(choice.slug)}
                      style={[
                        styles.row,
                        index > 0 && {
                          borderTopWidth: StyleSheet.hairlineWidth,
                          borderColor: colors.borderSubtle,
                        },
                      ]}
                    >
                      <View style={styles.name}>
                        <Text style={ink}>{choice.name}</Text>
                        {page === 'Model' && choice.slug === 'default' && (
                          <Text style={{ color: colors.textSecondary }}>
                            The model {props.provider} is configured with
                          </Text>
                        )}
                      </View>
                      {selected === choice.slug && (
                        <Check size={17} color={colors.textPrimary} />
                      )}
                    </Pressable>
                  ))}
                  {choices?.length === 0 && (
                    <Text style={[styles.status, ink]}>
                      No models available
                    </Text>
                  )}
                  {!choices && !error && (
                    <Text style={[styles.status, ink]}>Loading models…</Text>
                  )}
                </View>
                {!!error && (
                  <Text accessibilityRole="alert" style={[styles.status, ink]}>
                    {error}
                  </Text>
                )}
                {!!error && !choices && (
                  <Pressable accessibilityRole="button" onPress={open}>
                    <Text style={[styles.status, ink]}>Try again</Text>
                  </Pressable>
                )}
                {page === 'Model' && permission && (
                  <>
                    <Text
                      style={[styles.section, { color: colors.textSecondary }]}
                    >
                      Options
                    </Text>
                    <Pressable
                      accessibilityRole="button"
                      accessibilityLabel="Permissions"
                      onPress={() => setPage('Permissions')}
                      style={[
                        styles.group,
                        styles.row,
                        { backgroundColor: colors.bgGroup },
                      ]}
                    >
                      <Text style={[styles.name, ink]}>Permissions</Text>
                      <Text
                        style={{
                          color:
                            permission.slug === 'full-access'
                              ? colors.statusAmber
                              : colors.textSecondary,
                        }}
                      >
                        {permission.name}
                      </Text>
                      <ChevronRight size={16} color={colors.textSecondary} />
                    </Pressable>
                  </>
                )}
              </ScrollView>
            </View>
          </View>
        </Modal>
      )}
    </>
  );
}

const styles = StyleSheet.create({
  pill: {
    height: 38,
    paddingHorizontal: 14,
    borderRadius: 20,
    justifyContent: 'center',
    maxWidth: 180,
  },
  title: { fontSize: 15, fontWeight: '500' },
  overlay: {
    flex: 1,
    justifyContent: 'flex-end',
    backgroundColor: 'rgba(0,0,0,0.55)',
  },
  sheet: {
    maxHeight: '84%',
    paddingHorizontal: 12,
    borderTopLeftRadius: 20,
    borderTopRightRadius: 20,
  },
  handle: {
    width: 38,
    height: 5,
    alignSelf: 'center',
    borderRadius: 3,
    marginTop: 8,
    opacity: 0.45,
  },
  header: { height: 58, justifyContent: 'center', alignItems: 'center' },
  heading: { fontSize: 16, fontWeight: '600' },
  done: { position: 'absolute', right: 8, padding: 8 },
  back: { position: 'absolute', left: 4, padding: 8 },
  section: { fontSize: 13, marginLeft: 16, marginTop: 12, marginBottom: 8 },
  group: { borderRadius: 16, overflow: 'hidden' },
  row: {
    minHeight: 48,
    paddingHorizontal: 16,
    paddingVertical: 10,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  name: { flex: 1, gap: 2 },
  status: { padding: 14 },
});
