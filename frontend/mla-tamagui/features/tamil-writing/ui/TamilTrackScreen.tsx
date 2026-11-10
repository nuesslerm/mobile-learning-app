import { Logger } from '@lib/logging';
import { Badge, ScreenHeader, Text, View } from '@lib/tamagui-design-system';
import { router } from 'expo-router';
import { useEffect, useMemo } from 'react';
import { Pressable } from 'react-native';
import { ScrollView, XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { isMastered } from '../domain/mastery';
import { getTrack } from '../domain/tracks';
import { useTamilProgressStore } from '../state/tamilProgressStore';

export type TamilTrackScreenProps = {
  trackId: string;
};

/**
 * One track laid out as tiles: the glyph large, its name under it, a tick when
 * every letter in it is mastered. Tap a tile to write it.
 */
export function TamilTrackScreen({ trackId }: TamilTrackScreenProps) {
  const track = useMemo(() => getTrack(trackId), [trackId]);
  const mastery = useTamilProgressStore((state) => state.characters);

  const missing = !track;
  useEffect(() => {
    if (missing) Logger.info('tamil.track.not_found', { trackId });
  }, [missing, trackId]);

  if (!track) {
    return (
      <View flex={1} bg="$background" testID="TamilTrackMissing">
        <ScreenHeader
          title="Track not found"
          onBack={() => router.back()}
          testID="TamilTrackMissingHeader"
        />
        <YStack p="$4">
          <Text variant="muted" testID="TamilTrackMissingText">
            No track with that id.
          </Text>
        </YStack>
      </View>
    );
  }

  const isWord = track.kind === 'words';

  return (
    <View flex={1} bg="$background" testID="TamilTrackScreen">
      <ScreenHeader
        title={track.title}
        subtitle={track.subtitle}
        onBack={() => router.back()}
        testID="TamilTrackHeader"
      />
      <ScrollView flex={1} testID="TamilTrackScroll">
        <XStack flexWrap="wrap" px="$3" py="$3" gap="$2" testID="TamilTrackGrid">
          {track.subjects.map((subject) => {
            const done =
              subject.characterIds.length > 0 &&
              subject.characterIds.every((id) => isMastered(mastery[id]?.mastery ?? 0));
            return (
              <Pressable
                key={subject.id}
                testID="TamilTrackTile"
                role="button"
                aria-label={`Write ${subject.glyph}`}
                onPress={() => {
                  Logger.info('tamil.tracks.write', { trackId: track.id, subjectId: subject.id });
                  router.push(`/tamil/write/${subject.id}`);
                }}
              >
                <YStack
                  width={isWord ? 160 : 76}
                  height={isWord ? 96 : 96}
                  bg={done ? '$green3' : '$color2'}
                  borderWidth={1}
                  borderColor={done ? '$green7' : '$borderColor'}
                  rounded="$4"
                  items="center"
                  justify="center"
                  gap="$1"
                  px="$2"
                >
                  <Text
                    fontSize={isWord ? 26 : 34}
                    lineHeight={isWord ? 34 : 42}
                    numberOfLines={1}
                    testID="TamilTrackTileGlyph"
                  >
                    {subject.glyph}
                  </Text>
                  <Text variant="small" color="$color10" numberOfLines={1} testID="TamilTrackTileLabel">
                    {subject.transliteration}
                  </Text>
                  {done ? (
                    <Badge variant="success" testID="TamilTrackTileDone">
                      ✓
                    </Badge>
                  ) : null}
                </YStack>
              </Pressable>
            );
          })}
        </XStack>
      </ScrollView>
    </View>
  );
}
