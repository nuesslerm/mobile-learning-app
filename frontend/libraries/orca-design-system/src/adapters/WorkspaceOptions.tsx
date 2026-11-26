import { BottomSheet } from './BottomSheet';
import type {
  WorkspaceOptionGroup,
  WorkspaceOptionsProps,
} from '@lib/agent-ui-contracts';
import { Fragment, useState } from 'react';
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { CheckIcon, ChevronRightIcon, homeIcons } from './homeIcons';
import { WorkspaceSwitch } from './WorkspaceSwitch';
import { useChatPalette } from './chatPalette';

function OptionGroup({
  group,
  first,
  disabled,
}: {
  group: WorkspaceOptionGroup;
  first: boolean;
  disabled: boolean;
}) {
  const c = useChatPalette();
  return (
    <View>
      {group.label && (
        <Text
          style={[
            styles.section,
            first && styles.firstSection,
            { color: c.textSecondary },
          ]}
        >
          {group.label}
        </Text>
      )}
      <View
        style={[
          styles.group,
          { backgroundColor: c.bgGroup },
          group.segments && styles.segments,
        ]}
      >
        {group.rows.map((row, index) => {
          if (group.segments)
            return (
              <Pressable
                key={row.label}
                accessibilityRole="radio"
                accessibilityState={{ checked: row.selected, disabled }}
                disabled={disabled}
                onPress={row.onPress}
                style={[styles.segment, row.selected && styles.selectedSegment]}
              >
                <Text
                  numberOfLines={1}
                  style={[
                    styles.segmentText,
                    { color: row.selected ? c.textPrimary : c.textSecondary },
                  ]}
                >
                  {row.label}
                </Text>
              </Pressable>
            );
          const content = (
            <>
              <View style={styles.name}>
                <Text
                  style={[
                    styles.rowLabel,
                    { color: c.textPrimary },
                    row.description && styles.emphasis,
                  ]}
                >
                  {row.label}
                </Text>
                {row.description && (
                  <Text style={[styles.detail, { color: c.textSecondary }]}>
                    {row.description}
                  </Text>
                )}
              </View>
              {row.value !== undefined && (
                <Text style={[styles.rowLabel, { color: c.textSecondary }]}>
                  {row.value}
                </Text>
              )}
              {row.kind === 'page' && (
                <ChevronRightIcon size={16} color={c.textMuted} />
              )}
              {row.kind === 'switch' ? (
                <WorkspaceSwitch
                  label={row.label}
                  disabled={disabled}
                  checked={!!row.selected}
                  onPress={row.onPress}
                />
              ) : (
                row.selected && <CheckIcon size={17} color={c.textPrimary} />
              )}
            </>
          );
          const rowStyle = [styles.row, row.description && styles.twoLine];
          return (
            <Fragment key={row.label}>
              {index > 0 && (
                <View
                  style={[styles.rule, { backgroundColor: c.borderSubtle }]}
                />
              )}
              {row.kind === 'switch' ? (
                <View style={rowStyle}>{content}</View>
              ) : (
                <Pressable
                  accessibilityRole={row.kind === 'page' ? 'button' : row.kind}
                  accessibilityLabel={
                    row.value === undefined
                      ? row.label
                      : row.label + ', ' + row.value
                  }
                  aria-checked={row.selected}
                  accessibilityState={{
                    checked: row.selected,
                    disabled: disabled && row.kind !== 'page',
                  }}
                  disabled={disabled && row.kind !== 'page'}
                  onPress={row.onPress}
                  style={rowStyle}
                >
                  {content}
                </Pressable>
              )}
            </Fragment>
          );
        })}
      </View>
    </View>
  );
}

export function WorkspaceOptions(props: WorkspaceOptionsProps) {
  const colors = useChatPalette();
  const { top, bottom } = useSafeAreaInsets();
  const { height } = useWindowDimensions();
  const [rootHeight, setRootHeight] = useState(0);
  const maxHeight = height - top - 12;
  const Options = homeIcons.options;
  const Back = homeIcons.back;
  const ink = { color: colors.textPrimary };
  return (
    <>
      <Pressable
        accessibilityLabel="Workspace options"
        accessibilityRole="button"
        onPress={props.onOpen}
        style={[styles.trigger, { backgroundColor: colors.bgRaised }]}
      >
        <Options size={20} color={colors.textPrimary} />
      </Pressable>
      {props.visible && (
        <BottomSheet
          onClose={props.onClose}
          onBack={props.page ? props.onBack : undefined}
          style={[
            styles.sheet,
            {
              backgroundColor: colors.sheetSurface,
              maxHeight,
              paddingBottom: bottom + 28,
            },
            props.page && rootHeight > 0
              ? { minHeight: Math.min(rootHeight, maxHeight) }
              : undefined,
          ]}
          onLayout={(event) => {
            if (!props.page && !props.loading && props.groups.length)
              setRootHeight(event.nativeEvent.layout.height);
          }}
        >
          {(dismiss) => (
            <>
              <View
                style={[styles.handle, { backgroundColor: colors.textMuted }]}
              />
              <View style={styles.header}>
                {props.page ? (
                  <Pressable
                    style={styles.left}
                    accessibilityRole="button"
                    accessibilityLabel="Back to Workspace options"
                    onPress={props.onBack}
                  >
                    <Back size={19} color={colors.textPrimary} />
                  </Pressable>
                ) : (
                  <Pressable
                    style={styles.left}
                    accessibilityRole="button"
                    disabled={!props.resetEnabled || props.saving}
                    onPress={props.onReset}
                  >
                    <Text
                      style={[
                        styles.action,
                        ink,
                        !props.resetEnabled && styles.disabled,
                      ]}
                    >
                      Reset
                    </Text>
                  </Pressable>
                )}
                <Text accessibilityRole="header" style={[styles.title, ink]}>
                  {props.page ?? 'Workspace options'}
                </Text>
                {!props.page && (
                  <Text
                    style={[styles.subtitle, { color: colors.textSecondary }]}
                  >
                    Synced across your devices
                  </Text>
                )}
                <Pressable
                  style={styles.right}
                  accessibilityRole="button"
                  onPress={() => dismiss()}
                >
                  <Text style={[styles.title, ink]}>Done</Text>
                </Pressable>
              </View>
              <ScrollView
                key={props.page ?? 'root'}
                bounces={false}
                style={styles.scroll}
                contentContainerStyle={styles.content}
              >
                {props.loading && (
                  <Text style={[styles.notice, ink]}>
                    Loading workspace options…
                  </Text>
                )}
                {!!props.error && (
                  <View>
                    <Text
                      accessibilityRole="alert"
                      style={[styles.notice, ink]}
                    >
                      {props.error}
                    </Text>
                    <Pressable
                      accessibilityRole="button"
                      onPress={props.onRetry}
                    >
                      <Text style={[styles.notice, ink]}>Try again</Text>
                    </Pressable>
                  </View>
                )}
                {(props.page ? props.pageGroups : props.groups).map(
                  (group, index) => (
                    <OptionGroup
                      first={index === 0}
                      key={group.label ?? group.rows[0]?.label}
                      group={group}
                      disabled={props.saving}
                    />
                  )
                )}
              </ScrollView>
            </>
          )}
        </BottomSheet>
      )}
    </>
  );
}

const styles = StyleSheet.create({
  trigger: {
    alignSelf: 'flex-start',
    height: 44,
    width: 44,
    margin: 12,
    borderRadius: 22,
    alignItems: 'center',
    justifyContent: 'center',
  },
  sheet: {
    paddingHorizontal: 12,
    borderTopLeftRadius: 24,
    borderTopRightRadius: 24,
  },
  handle: {
    alignSelf: 'center',
    width: 36,
    height: 5,
    borderRadius: 3,
    marginTop: 8,
    marginBottom: 12,
    opacity: 0.5,
  },
  header: { height: 52, alignItems: 'center', justifyContent: 'center' },
  title: { fontSize: 16, fontWeight: '600' },
  action: { fontSize: 16 },
  subtitle: { fontSize: 12 },
  left: {
    position: 'absolute',
    left: 4,
    minWidth: 44,
    height: 44,
    paddingHorizontal: 12,
    justifyContent: 'center',
  },
  right: {
    position: 'absolute',
    right: 4,
    height: 44,
    paddingHorizontal: 12,
    justifyContent: 'center',
  },
  disabled: { opacity: 0.38 },
  scroll: { flexGrow: 0 },
  content: { gap: 16 },
  section: {
    fontSize: 13,
    fontWeight: '500',
    marginLeft: 16,
    marginBottom: 6,
    marginTop: 12,
    minHeight: 17,
  },
  firstSection: { marginTop: 4 },
  group: { borderRadius: 16, overflow: 'hidden' },
  segments: {
    flexDirection: 'row',
    padding: 3,
    borderRadius: 12,
    minHeight: 40,
  },
  segment: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
    borderRadius: 9,
  },
  selectedSegment: { backgroundColor: 'rgba(120,120,128,0.16)' },
  segmentText: { fontSize: 13, fontWeight: '600' },
  row: {
    minHeight: 48,
    paddingHorizontal: 16,
    paddingVertical: 8,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
  },
  rule: { marginLeft: 16, height: StyleSheet.hairlineWidth },
  twoLine: { minHeight: 60 },
  name: { flex: 1, gap: 2 },
  rowLabel: { fontSize: 15 },
  detail: { fontSize: 13 },
  emphasis: { fontWeight: '500' },
  notice: { fontSize: 14, padding: 12 },
});
