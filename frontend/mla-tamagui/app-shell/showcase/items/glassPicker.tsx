import { Button, Icon } from '@lib/tamagui-design-system';
import { Check, ChevronDown, Hand, Settings, ShieldAlert, ShieldCheck } from '@lib/tamagui-design-system/icons';
import { Theme, XStack } from '@lib/tamagui-design-system/primitives';
import { Sheet, SheetText } from '@lib/tamagui-design-system/ui/sheet';
import { Link } from 'expo-router';
import { useState } from 'react';
import { Platform, Pressable, StyleSheet, View, useWindowDimensions } from 'react-native';

import { Overlay } from '../overlayHost';

const swiftUI = Platform.OS === 'ios'
  ? (require('@expo/ui/swift-ui') as typeof import('@expo/ui/swift-ui'))
  : null;

const APPROVALS = [
  {
    id: 'ask',
    label: 'Ask for approval',
    detail: 'Always ask to edit external files and use\nthe internet',
    symbol: 'hand.raised',
    icon: Hand,
  },
  {
    id: 'automatic',
    label: 'Approve for me',
    detail: 'Only ask for actions detected as\npotentially unsafe',
    symbol: 'shield',
    icon: ShieldCheck,
  },
  {
    id: 'full',
    label: 'Full access',
    detail: 'Full computer access (elevated risk)',
    symbol: 'exclamationmark.shield',
    icon: ShieldAlert,
  },
  {
    id: 'custom',
    label: 'Custom (config.toml)',
    detail: 'Uses permissions defined in config.toml',
    symbol: 'gearshape',
    icon: Settings,
  },
] as const;

const LINK = 'https://developers.openai.com/codex/agent-approvals-security';
type Approval = (typeof APPROVALS)[number];

function ApprovalSymbol({ symbol, icon, size = 20, color = '#F9F9FB' }: {
  symbol: Approval['symbol'] | 'checkmark';
  icon: Approval['icon'];
  size?: number;
  color?: string;
}) {
  if (swiftUI) {
    return (
      <View style={styles.symbol}>
        <swiftUI.Host matchContents>
          <swiftUI.Image systemName={symbol} size={size} color={color} />
        </swiftUI.Host>
        {symbol === 'shield' && (
          <View style={styles.shieldFace}>
            <View style={styles.shieldMarks}><View style={styles.shieldDot} /><View style={styles.shieldDot} /></View>
            <View style={styles.shieldDash} />
          </View>
        )}
      </View>
    );
  }
  return <Icon as={icon} size={size} color={color} />;
}

export function GlassPickerItem() {
  const [open, setOpen] = useState(false);
  const [selection, setSelection] = useState<Approval>(APPROVALS[1]);
  const { width, height } = useWindowDimensions();
  const snapPoint = Math.min(92, 100 * (width * (1317 / 1284)) / Math.max(1, height));

  return (
    <>
      <Button
        variant="outline"
        rounded="$7"
        justify="space-between"
        testID="ShowcaseOpenApprovals"
        aria-expanded={open}
        onPress={() => setOpen(true)}
      >
        <SheetText style={styles.triggerLabel}>Approval mode</SheetText>
        <XStack items="center" gap="$2">
          <SheetText testID="ShowcaseApprovalValue" style={styles.triggerLabel}>{selection.label}</SheetText>
          <Icon as={ChevronDown} size={16} />
        </XStack>
      </Button>
      <View pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={styles.preview}>
        <View style={styles.previewBand} />
        <View style={styles.previewMiddleBand} />
        <View style={styles.previewLowerBand} />
        <View style={styles.previewAccent} />
      </View>
      <Overlay id="showcase-glass-picker">
        <Theme name="dark">
          <Sheet
            accessible={false}
            floating
            floatingHorizontalInset={Math.round(width * (26 / 1284))}
            floatingBottomInset={Math.round(height * (26 / 2778))}
            cornerRadius={34}
            backdropOpacity={0.43}
            glassColorScheme="dark"
            glassTintColor="rgba(31, 31, 33, 0.36)"
            open={open}
            onOpenChange={setOpen}
            snapPoints={[snapPoint]}
            handleLabel="Dismiss approval mode"
            handleIndicatorStyle={styles.indicator}
            contentStyle={styles.content}
            testID="ShowcaseApprovalSheet"
          >
            <View style={styles.header}>
              <SheetText accessibilityRole="header" style={styles.title}>
                How should Codex actions be approved?
              </SheetText>
              <Link href={LINK} target="_blank" style={styles.link}>Learn more</Link>
            </View>
            <Sheet.ScrollView showsVerticalScrollIndicator={false}>
              <View
                {...(Platform.OS === 'web' ? { role: 'radiogroup', 'aria-label': 'Approval mode' } : {})}
                style={styles.group}
              >
                {APPROVALS.map((option, index) => {
                  const checked = selection.id === option.id;
                  const elevated = option.id === 'full';
                  return (
                    <View key={option.id}>
                      <Pressable
                        accessibilityRole="radio"
                        accessibilityLabel={option.label}
                        accessibilityHint={option.detail.replace('\n', ' ')}
                        accessibilityState={{ checked }}
                        {...(Platform.OS === 'web'
                          ? { role: 'radio', 'aria-label': option.label, 'aria-checked': checked }
                          : {})}
                        testID={`ShowcaseApproval-${option.id}`}
                        onPress={() => setSelection(option)}
                        style={({ pressed }) => [
                          styles.row,
                          index >= 2 && styles.compactRow,
                          pressed && styles.pressedRow,
                        ]}
                      >
                        <View style={styles.leading}>
                          <ApprovalSymbol symbol={option.symbol} icon={option.icon} color={elevated ? '#FF9C65' : undefined} />
                        </View>
                        <View style={styles.rowText}>
                          <SheetText style={[styles.label, elevated && styles.elevated]}>{option.label}</SheetText>
                          <SheetText style={[styles.detail, elevated && styles.elevatedDetail]}>{option.detail}</SheetText>
                        </View>
                        <View style={styles.trailing}>
                          {checked && <ApprovalSymbol symbol="checkmark" icon={Check} size={17} />}
                        </View>
                      </Pressable>
                      {index < APPROVALS.length - 1 && <View style={styles.divider} />}
                    </View>
                  );
                })}
              </View>
            </Sheet.ScrollView>
          </Sheet>
        </Theme>
      </Overlay>
    </>
  );
}

const styles = StyleSheet.create({
  triggerLabel: { color: '#9A9AA0', fontSize: 15 },
  preview: { height: 458, marginTop: 16, position: 'relative' },
  previewBand: {
    position: 'absolute',
    left: 0,
    right: 0,
    top: 91,
    height: 62,
    borderRadius: 24,
    backgroundColor: '#3A3A3A',
  },
  previewMiddleBand: {
    position: 'absolute',
    left: 6,
    right: 5,
    top: 190,
    height: 42,
    borderRadius: 22,
    backgroundColor: '#272727',
  },
  previewLowerBand: {
    position: 'absolute',
    left: 0,
    right: 0,
    top: 348,
    height: 83,
    borderRadius: 24,
    backgroundColor: '#252525',
  },
  previewAccent: {
    position: 'absolute',
    right: 6,
    top: 381,
    height: 42,
    width: 34,
    borderRadius: 12,
    backgroundColor: '#3053AD',
  },
  indicator: {
    width: 58,
    height: 3,
    backgroundColor: 'rgba(224, 224, 229, 0.43)',
    transform: [{ translateY: -4 }],
  },
  content: { gap: 15, paddingHorizontal: 24, paddingTop: 1, paddingBottom: 16 },
  header: { alignItems: 'center' },
  title: { color: '#F7F7F9', textAlign: 'center', fontSize: 15, lineHeight: 20, fontWeight: '500' },
  link: { color: '#379AFF', fontSize: 15, lineHeight: 19 },
  group: {
    backgroundColor: 'rgba(255, 255, 255, 0.048)',
    borderRadius: 25,
    overflow: 'hidden',
  },
  row: {
    minHeight: 90,
    flexDirection: 'row',
    gap: 13,
    paddingHorizontal: 13,
    paddingVertical: 8,
    alignItems: 'center',
  },
  compactRow: { minHeight: 68 },
  pressedRow: { backgroundColor: 'rgba(255, 255, 255, 0.09)' },
  symbol: { alignItems: 'center', justifyContent: 'center' },
  shieldFace: { position: 'absolute', top: 6, alignItems: 'center', gap: 2 },
  shieldMarks: { flexDirection: 'row', gap: 3 },
  shieldDot: { width: 2, height: 2, backgroundColor: '#F9F9FB' },
  shieldDash: { width: 4, height: 1, backgroundColor: '#F9F9FB' },
  leading: { width: 24, alignItems: 'center' },
  rowText: { flex: 1, minWidth: 0, gap: 1 },
  label: { color: '#F5F5F7', fontSize: 16, lineHeight: 20 },
  detail: { color: '#AAAAAE', fontSize: 14, lineHeight: 18 },
  elevated: { color: '#FFA16B' },
  elevatedDetail: { color: '#FF955D' },
  trailing: { width: 20, alignItems: 'center' },
  divider: {
    marginLeft: 50,
    height: StyleSheet.hairlineWidth,
    backgroundColor: 'rgba(255, 255, 255, 0.13)',
  },
});
