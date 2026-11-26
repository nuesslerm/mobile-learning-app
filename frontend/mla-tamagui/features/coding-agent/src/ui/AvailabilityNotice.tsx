import { useChatPalette } from '@lib/orca-design-system';
import { Pressable, StyleSheet, Text, View } from 'react-native';

export function AvailabilityNotice({ missing, reconnecting, onAction }: {
  missing?: boolean;
  reconnecting?: boolean;
  onAction: () => void;
}) {
  const colors = useChatPalette();
  return (
    <View style={[styles.card, { backgroundColor: colors.bgRaised }]}>
      <View accessibilityLiveRegion="polite" accessibilityRole="alert">
        <Text style={[styles.title, { color: colors.textPrimary }]}>
          {missing ? 'Worktree unavailable' : 'RCA server unreachable'}
        </Text>
        <Text style={[styles.detail, { color: colors.textSecondary }]}>
          {missing ? 'This worktree may have been deleted or moved on the desktop.'
            : 'Check that the desktop is online and RCA is running.' + (reconnecting ? ' Reconnecting automatically.' : '')}
        </Text>
      </View>
      <Pressable accessibilityRole="button" onPress={onAction} style={styles.action}>
        <Text style={[styles.label, { color: colors.accentBlue }]}>{missing ? 'View workspaces' : 'Reconnect now'}</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  card: { marginHorizontal: 12, marginVertical: 10, paddingHorizontal: 14, paddingTop: 14, borderRadius: 14 },
  title: { fontSize: 16, fontWeight: '600' },
  detail: { fontSize: 14, lineHeight: 20, marginTop: 4 },
  action: { alignSelf: 'flex-start', minHeight: 44, justifyContent: 'center' },
  label: { fontSize: 15, fontWeight: '600' },
});
