import { Logger } from '@lib/logging';
import { Badge, InsetGroup, ListItem, ScreenHeader, Text, View } from '@lib/tamagui-design-system';
import { router } from 'expo-router';
import { useMemo } from 'react';
import { ScrollView, XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { isMastered } from '../domain/mastery';
import { TRACKS, type Track } from '../domain/tracks';
import { useTamilProgressStore } from '../state/tamilProgressStore';

import { MasteryBar } from './MasteryBar';

/**
 * The track list: what there is to write, and how far along each is.
 *
 * Letters and words sit side by side because that is how a learner moves --
 * a few vowels, then a word made of them, then back. Progress is read off the
 * same mastery ladder the lessons feed, so writing அ large here and tracing it
 * there count towards the same thing.
 */
export function TamilTracksScreen() {
  const mastery = useTamilProgressStore((state) => state.characters);

  const progress = useMemo(() => {
    const out = new Map<string, { done: number; total: number }>();
    for (const track of TRACKS) {
      let done = 0;
      for (const subject of track.subjects) {
        const ids = subject.characterIds;
        if (ids.length > 0 && ids.every((id) => isMastered(mastery[id]?.mastery ?? 0))) {
          done += 1;
        }
      }
      out.set(track.id, { done, total: track.subjects.length });
    }
    return out;
  }, [mastery]);

  const groups: { kind: Track['kind']; title: string; tracks: Track[] }[] = [
    { kind: 'letters', title: 'எழுத்துக்கள் · Letters', tracks: TRACKS.filter((t) => t.kind === 'letters') },
    { kind: 'words', title: 'சொற்கள் · Words', tracks: TRACKS.filter((t) => t.kind === 'words') },
  ];

  return (
    <View flex={1} bg="$background" testID="TamilTracksScreen">
      <ScreenHeader
        title="Write it large"
        subtitle="Pick a track"
        onBack={() => router.back()}
        testID="TamilTracksHeader"
      />
      <ScrollView flex={1} testID="TamilTracksScroll">
        {groups.map((group) => (
          <YStack key={group.kind} mt="$4">
            <Text variant="h3" mx="$4" mb="$2" testID="TamilTracksSectionTitle">
              {group.title}
            </Text>
            <InsetGroup testID={`TamilTracks-${group.kind}`}>
              {group.tracks.map((track) => {
                const p = progress.get(track.id) ?? { done: 0, total: track.subjects.length };
                return (
                  <ListItem
                    key={track.id}
                    testID="TamilTrackRow"
                    variant="grouped"
                    showChevron
                    pressLog="off"
                    onPress={() => {
                      Logger.info('tamil.tracks.open', { trackId: track.id });
                      router.push(`/tamil/tracks/${track.id}`);
                    }}
                  >
                    <XStack items="center" gap="$3" flex={1}>
                      <YStack flex={1} gap="$0.5">
                        <Text variant="strong" testID="TamilTrackRowTitle">
                          {track.title}
                        </Text>
                        <Text variant="small" color="$color10" testID="TamilTrackRowSubtitle">
                          {track.subtitle}
                        </Text>
                        <MasteryBar
                          ratio={p.total ? p.done / p.total : 0}
                          testID="TamilTrackRowBar"
                        />
                      </YStack>
                      <Badge
                        variant={p.done === p.total ? 'success' : 'outline'}
                        testID="TamilTrackRowCount"
                      >
                        {p.done}/{p.total}
                      </Badge>
                    </XStack>
                  </ListItem>
                );
              })}
            </InsetGroup>
          </YStack>
        ))}
      </ScrollView>
    </View>
  );
}
