import type { HomeAgentSectionProps } from '@lib/skin';
import Bell from 'lucide-react-native/dist/esm/icons/bell.mjs';
import { Fragment } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { homePalettes, useOrcaScheme } from './appearance';

export function HomeAgentSection({
  title,
  rows,
  onSelect,
}: HomeAgentSectionProps) {
  const c = homePalettes[useOrcaScheme()];
  const muted = { color: c.textMuted };
  return (
    <View style={styles.section}>
      <Text
        accessibilityRole="header"
        style={[styles.heading, { color: c.textPrimary }]}
      >
        {title} <Text style={[styles.count, muted]}>{rows.length}</Text>
      </Text>
      <View style={[styles.group, { backgroundColor: c.bgPanel }]}>
        {rows.map((row, index) => {
          const dot = {
            interrupted: c.textMuted,
            done: '#10b981',
            working: '#eab308',
            waiting: '#ef4444',
            blocked: '#ef4444',
            failed: '#ef4444',
          }[row.status];
          return (
            <Fragment key={row.id}>
              {index > 0 && (
                <View
                  style={[
                    styles.separator,
                    { backgroundColor: c.borderSubtle },
                  ]}
                />
              )}
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={row.title + ', ' + row.place}
                onPress={() => onSelect(row.id)}
                style={styles.row}
              >
                <View style={styles.status}>
                  <View style={[styles.dot, { backgroundColor: dot }]} />
                </View>
                <View style={styles.details}>
                  <View style={styles.line}>
                    <Text
                      numberOfLines={1}
                      style={[
                        styles.title,
                        { color: c.textPrimary },
                        row.unread && styles.unread,
                      ]}
                    >
                      {row.title}
                    </Text>
                    <Text style={[styles.age, muted]}>{row.age}</Text>
                  </View>
                  <View style={styles.line}>
                    <Text numberOfLines={1} style={[styles.place, muted]}>
                      {row.place}
                    </Text>
                    {row.unread && (
                      <Bell
                        size={12}
                        color={c.statusAmber}
                        fill={c.statusAmber}
                      />
                    )}
                  </View>
                </View>
              </Pressable>
            </Fragment>
          );
        })}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  section: { paddingHorizontal: 16, paddingTop: 4, paddingBottom: 12 },
  heading: {
    fontSize: 15,
    fontWeight: '600',
    marginHorizontal: 4,
    marginBottom: 7,
  },
  count: { fontWeight: '500', fontVariant: ['tabular-nums'] },
  group: { borderRadius: 16, overflow: 'hidden' },
  separator: { marginLeft: 40, height: StyleSheet.hairlineWidth },
  row: { flexDirection: 'row', paddingLeft: 16 },
  status: {
    width: 24,
    height: 20,
    marginTop: 9,
    paddingLeft: 2,
    justifyContent: 'center',
  },
  dot: { width: 8, height: 8, borderRadius: 4 },
  details: {
    flex: 1,
    minWidth: 0,
    paddingTop: 9,
    paddingBottom: 10,
    paddingRight: 14,
    gap: 3,
  },
  line: { flexDirection: 'row', alignItems: 'baseline', gap: 8 },
  title: { flex: 1, minWidth: 0, fontSize: 15, lineHeight: 20 },
  unread: { fontWeight: '600' },
  age: { fontSize: 12, fontVariant: ['tabular-nums'] },
  place: { flex: 1, fontSize: 12 },
});
