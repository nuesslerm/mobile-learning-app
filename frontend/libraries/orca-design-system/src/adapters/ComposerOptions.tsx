import type { ComposerOptionsProps } from '@lib/agent-ui-contracts';
import { Fragment, useCallback, useEffect, useRef, useState } from 'react';
import Check from 'lucide-react-native/dist/esm/icons/check.mjs';
import ChevronLeft from 'lucide-react-native/dist/esm/icons/chevron-left.mjs';
import ChevronRight from 'lucide-react-native/dist/esm/icons/chevron-right.mjs';
import FilePenLine from 'lucide-react-native/dist/esm/icons/file-pen-line.mjs';
import Hand from 'lucide-react-native/dist/esm/icons/hand.mjs';
import ShieldAlert from 'lucide-react-native/dist/esm/icons/shield-alert.mjs';
import ShieldCheck from 'lucide-react-native/dist/esm/icons/shield-check.mjs';
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
import { useReducedMotionEnabled } from './useReducedMotion';
import { useChatPalette } from './chatPalette';

const permissionIcons = new Map([
  ['approval-required', Hand],
  ['auto-accept-edits', FilePenLine],
  ['auto', ShieldCheck],
  ['full-access', ShieldAlert],
]);

type Choice = { slug: string; name: string };

export function ComposerOptions(props: ComposerOptionsProps) {
  const colors = useChatPalette();
  const reducedMotion = useReducedMotionEnabled();
  const { bottom } = useSafeAreaInsets();
  const [page, setPage] = useState<'Model' | 'Permissions' | 'Effort'>('Model');
  const [models, setModels] = useState<readonly Choice[]>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const operation = useRef(0);
  const { visible, loadModels, onVisibleChange } = props;
  const opensEffort = Boolean(props.effort);
  const ink = { color: colors.textPrimary };
  const load = useCallback(() => {
    setError('');
    setModels(undefined);
    const request = ++operation.current;
    void loadModels().then(
      (choices) => {
        if (request === operation.current) setModels(choices);
      },
      () => {
        if (request === operation.current) setError('Could not load models.');
      }
    );
  }, [loadModels]);
  useEffect(() => {
    if (!visible) return;
    Keyboard.dismiss();
    setPage(opensEffort ? 'Effort' : 'Model');
    setBusy(false);
    setError('');
    if (!opensEffort) load();
    return () => { operation.current += 1; };
  }, [visible, load, opensEffort]);
  function returnToModels() {
    if (page === 'Effort') load();
    setPage('Model');
  }
  function close() {
    onVisibleChange(false);
  }
  function select(slug: string) {
    if (busy) return;
    setError('');
    setBusy(true);
    const request = ++operation.current;
    const save = page === 'Effort' ? props.effort?.select : page === 'Model' ? props.setModel : props.setPermission;
    if (!save) { setBusy(false); return; }
    void save(slug).then(
      () => {
        if (request !== operation.current) return;
        setBusy(false);
        returnToModels();
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
  const choices = page === 'Effort' ? props.effort?.choices : page === 'Permissions' ? props.permissions : models;
  const selected =
    page === 'Effort' ? props.effort?.current : page === 'Permissions' ? props.permission : props.selectedModel;
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={
          'Model, ' +
          label +
          (props.permission === 'full-access' ? ', full access' : '')
        }
        onPress={() => onVisibleChange(true)}
        style={[styles.pill, { backgroundColor: colors.bgRaised }]}
      >
        {props.permission === 'full-access' && (
          <ShieldAlert size={15} color={colors.statusAmber} strokeWidth={2.2} />
        )}
        <Text numberOfLines={1} style={[styles.title, ink]}>
          {label}
        </Text>
      </Pressable>
      {visible && (
        <Modal
          visible
          transparent
          animationType={reducedMotion ? 'fade' : 'slide'}
          onRequestClose={close}
        >
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
                  paddingBottom: bottom + 20,
                  backgroundColor: colors.sheetSurface,
                },
              ]}
            >
              <View
                style={[styles.handle, { backgroundColor: colors.textMuted }]}
              />
              <View style={styles.header}>
                {page !== 'Model' && (
                  <Pressable
                    accessibilityRole="button"
                    accessibilityLabel="Back to models"
                    style={styles.back}
                    onPress={returnToModels}
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
                {page === 'Model' ? (
                  <Text
                    style={[
                      styles.section,
                      styles.firstSection,
                      { color: colors.textSecondary },
                    ]}
                  >
                    Model
                  </Text>
                ) : (
                  <View style={{ height: 8 }} />
                )}
                <View
                  style={[styles.group, { backgroundColor: colors.bgGroup }]}
                >
                  {choices?.map((choice, index) => {
                    const Icon =
                      page === 'Permissions'
                        ? permissionIcons.get(choice.slug)
                        : undefined;
                    const subtitle =
                      page === 'Model' && choice.slug === 'default';
                    return (
                      <Fragment key={choice.slug}>
                        {index > 0 && (
                          <View
                            style={[
                              styles.separator,
                              {
                                marginLeft: Icon ? 52 : 16,
                                backgroundColor: colors.borderSubtle,
                              },
                            ]}
                          />
                        )}
                        <Pressable
                          accessibilityRole="button"
                          accessibilityState={{
                            selected: selected === choice.slug,
                            disabled: busy,
                          }}
                          disabled={busy}
                          onPress={() => select(choice.slug)}
                          style={[styles.row, subtitle && styles.twoLine]}
                        >
                          {Icon && (
                            <View style={styles.leadingIcon}>
                              <Icon
                                size={18}
                                strokeWidth={2.2}
                                color={
                                  choice.slug === 'full-access'
                                    ? colors.statusAmber
                                    : colors.textSecondary
                                }
                              />
                            </View>
                          )}
                          <View style={styles.name}>
                            <Text
                              style={[
                                styles.rowTitle,
                                ink,
                                subtitle && styles.emphasis,
                              ]}
                            >
                              {choice.name}
                            </Text>
                            {subtitle && (
                              <Text
                                style={[
                                  styles.subtitle,
                                  { color: colors.textSecondary },
                                ]}
                              >
                                The model {props.provider} is configured with
                              </Text>
                            )}
                          </View>
                          {selected === choice.slug && (
                            <Check size={17} color={colors.textPrimary} />
                          )}
                        </Pressable>
                      </Fragment>
                    );
                  })}
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
                  <Pressable accessibilityRole="button" onPress={load}>
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
                      <Text style={[styles.name, styles.rowTitle, ink]}>
                        Permissions
                      </Text>
                      <Text
                        style={{
                          fontSize: 15,
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
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
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
    borderTopLeftRadius: 24,
    borderTopRightRadius: 24,
  },
  handle: {
    width: 36,
    height: 5,
    alignSelf: 'center',
    borderRadius: 3,
    marginTop: 8,
    opacity: 0.5,
  },
  header: {
    marginTop: 8,
    height: 52,
    justifyContent: 'center',
    alignItems: 'center',
  },
  heading: { fontSize: 16, fontWeight: '600' },
  done: {
    position: 'absolute',
    right: 4,
    minHeight: 44,
    paddingHorizontal: 12,
    justifyContent: 'center',
  },
  back: { position: 'absolute', left: 4, padding: 8 },
  section: {
    fontSize: 13,
    fontWeight: '500',
    marginLeft: 16,
    marginTop: 16,
    marginBottom: 6,
  },
  firstSection: { marginTop: 4 },
  group: { borderRadius: 16, overflow: 'hidden' },
  row: {
    minHeight: 48,
    paddingHorizontal: 16,
    paddingVertical: 8,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
  },
  leadingIcon: { width: 24, alignItems: 'center' },
  separator: { height: StyleSheet.hairlineWidth },
  rowTitle: { fontSize: 15 },
  emphasis: { fontWeight: '500' },
  subtitle: { fontSize: 13 },
  twoLine: { minHeight: 60 },
  name: { flex: 1, gap: 2 },
  status: { padding: 14 },
});
