import { useRouteNavigation } from '../ui/useRouteAction';
import { CompactIconButton, Icon, useCompactColors } from '@lib/tamagui-design-system';
import { ChevronLeft, Hand, Share as ShareIcon } from '@lib/tamagui-design-system/icons';
import { router, type Href } from 'expo-router';
import { useEffect, useState } from 'react';
import { Image, Linking, Pressable, ScrollView, Share, StyleSheet, Text, useWindowDimensions, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { libraryArtwork } from '../ui/library/libraryArtwork';
import { LibraryDock } from '../ui/library/LibraryDock';

const contestUrl = 'https://leetcode.com/contest/weekly-contest-530/';
const prizes = [
  ['1st - 3rd', 'LeetCode Backpack'],
  ['4th - 10th', 'LeetCode Water Bottle'],
  ['30th, 530th, 1024th, 1337th, and 2048th', 'LeetCode Big O Notebook'],
];

function ContestClock({ color }: { color: string }) {
  const starts = Date.parse('2026-11-29T02:30:00Z');
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (Date.now() >= starts) return;
    const clock = setInterval(() => {
      const next = Date.now();
      setNow(next);
      if (next >= starts) clearInterval(clock);
    }, 1000);
    return () => clearInterval(clock);
  }, [starts]);
  const seconds = Math.max(0, Math.ceil((starts - now) / 1000));
  const time = [Math.floor(seconds / 3600) % 24, Math.floor(seconds / 60) % 60, seconds % 60].map((value) => String(value).padStart(2, '0')).join(':');
  return <Text style={[styles.metadata, { color }]}>{seconds ? `●  Starts in ${Math.floor(seconds / 86400)}d ${time}` : 'Contest started'}</Text>;
}

export default function WeeklyContestRoute() {
  const { leave, pushRoute } = useRouteNavigation();
  const colors = useCompactColors();
  const { top, bottom } = useSafeAreaInsets();
  const { width } = useWindowDimensions();
  return <View style={{ flex: 1, backgroundColor: colors.background }}>
    <ScrollView testID="LibraryContest" contentContainerStyle={{ paddingBottom: bottom + 150 }}>
      <View style={{ backgroundColor: '#000', paddingTop: width * 0.445 }}>
        <Image source={libraryArtwork.contestDetail} style={{ position: 'absolute', top: 0, width, height: width * 0.65 }} />
        <View style={[styles.actions, { top: Math.min(top, width * 0.108) }]}>
          <CompactIconButton label="Back to Library" icon={ChevronLeft} onPress={() => leave(() => router.canGoBack() ? router.back() : router.replace('/'))} />
          <CompactIconButton label="Share contest" icon={ShareIcon} onPress={() => { void Share.share({ url: contestUrl, message: contestUrl }); }} />
        </View>
        <Text accessibilityRole="header" style={styles.title}>{'Weekly\nContest 530'}</Text>
      </View>
      <View style={styles.body}>
        <Text style={[styles.metadata, { color: colors.muted }]}>Sun, Nov 29, 03:30 CET</Text>
        <ContestClock color={colors.muted} />
        <Pressable accessibilityRole="link" onPress={() => { void Linking.openURL(contestUrl); }} style={[styles.register, { borderColor: colors.gold, backgroundColor: colors.gold + '18' }]}>
          <Icon as={Hand} size={20} color={colors.gold} /><Text style={{ fontSize: 17, fontWeight: '600', color: colors.gold }}>Register</Text>
        </Pressable>
        <Text style={[styles.heading, { color: colors.foreground }]}>{'Welcome to the 530th LeetCode\nWeekly Contest'}</Text>
        <Text style={[styles.paragraph, { color: colors.foreground }]}>This LeetCode contest is sponsored by LeetCode.</Text>
        <Text style={[styles.heading, { color: colors.foreground }]}>⭐ Bonus Prizes</Text>
        {prizes.map(([rank, prize]) => <View key={rank} style={styles.prize}>
          <Text style={{ color: colors.muted }}>•</Text><Text style={{ flex: 1, fontSize: 16, color: colors.foreground }}>Contestants ranked <Text style={{ fontWeight: '600' }}>{rank}</Text> will win a {prize}</Text>
        </View>)}
        <Text style={[styles.paragraph, { color: colors.foreground }]}>Only LCUS (leetcode.com) accounts are eligible for the bonus prizes. The rankings above determine the prize awarded to each winner.</Text>
        <View accessibilityLabel="Contest prizes" style={styles.rewards}>
          {['💼', '🧴', '📓'].map((symbol) => <View key={symbol} style={styles.reward}><Text style={{ fontSize: 32 }}>{symbol}</Text></View>)}
        </View>
      </View>
    </ScrollView>
    <LibraryDock pushRoute={pushRoute} onLibrary={() => leave(() => router.dismissTo('/'))} onSearch={() => pushRoute({ pathname: '/leetcode/library/collection', params: { collection: 'All Topics', search: '1' } } as Href)} />
  </View>;
}

const styles = StyleSheet.create({
  rewards: { flexDirection: 'row', gap: 14, marginTop: 40 },
  reward: { width: 64, height: 64, borderRadius: 32, backgroundColor: '#f6f0e9', borderColor: '#f2bb65', borderWidth: 2, alignItems: 'center', justifyContent: 'center' },
  actions: { position: 'absolute', left: 20, right: 20, flexDirection: 'row', justifyContent: 'space-between' },
  title: { color: '#fff', textAlign: 'center', fontSize: 40, fontWeight: '500', lineHeight: 47 },
  body: { paddingHorizontal: 20 },
  metadata: { fontSize: 16, textAlign: 'center', marginTop: 8 },
  register: { marginTop: 38, marginBottom: 23, borderRadius: 26, minHeight: 44, borderWidth: StyleSheet.hairlineWidth, flexDirection: 'row', gap: 8, alignItems: 'center', justifyContent: 'center' },
  heading: { marginTop: 12, fontSize: 20, fontWeight: '600' },
  paragraph: { marginTop: 12, fontSize: 16.5 },
  prize: { flexDirection: 'row', gap: 9, paddingHorizontal: 18, marginTop: 20 },
});
