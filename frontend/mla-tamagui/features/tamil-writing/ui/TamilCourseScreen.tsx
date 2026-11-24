import { Logger } from '@lib/logging';
import { Badge, Button, IconButton, InsetGroup, ListItem, Text, View } from '@lib/tamagui-design-system';
import { CollapsingHeader } from '@lib/tamagui-design-system/composites/collapsing-header';
import { Menu } from '@lib/tamagui-design-system/icons';
import { router, useNavigation } from 'expo-router';
import { Fragment, useCallback, useMemo } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { TamilCourse } from '../domain/course';
import { lessonProgress, type LessonProgress } from '../domain/mastery';
import type { TamilLesson } from '../domain/lessons';
import { useTamilProgressStore } from '../state/tamilProgressStore';

import { MasteryBar } from './MasteryBar';

const CONTENT_STYLE = { paddingBottom: 24 } as const;

function progressBadge(progress: LessonProgress) {
  if (progress.mastered === progress.total && progress.total > 0) {
    return { variant: 'success' as const, label: 'MASTERED' };
  }
  if (progress.started > 0) {
    return { variant: 'warning' as const, label: 'IN PROGRESS' };
  }
  return { variant: 'outline' as const, label: 'NEW' };
}

function LessonRow({
  lesson,
  progress,
}: {
  lesson: TamilLesson;
  progress: LessonProgress;
}) {
  const badge = progressBadge(progress);

  return (
    <ListItem
      testID="TamilLessonRow"
      variant="grouped"
      showChevron
      pressLog="off"
      onPress={() => {
        Logger.info('tamil.course.open_lesson', {
          lessonId: lesson.id,
          mastered: progress.mastered,
          total: progress.total,
        });
        router.push(`/tamil/${lesson.id}`);
      }}
    >
      <XStack items="center" gap="$3">
        <Text variant="h3" testID="TamilLessonRowGlyph">
          {lesson.title.slice(0, 1)}
        </Text>
        <YStack flex={1} gap="$1">
          <Text numberOfLines={1} testID="TamilLessonRowTitle">
            {lesson.title}
          </Text>
          <Text variant="muted" numberOfLines={1} testID="TamilLessonRowSubtitle">
            {lesson.subtitle}
          </Text>
        </YStack>
        <Badge variant={badge.variant} testID="TamilLessonRowBadge">
          {badge.label}
        </Badge>
      </XStack>
      <XStack items="center" gap="$3" mt="$1">
        <View flex={1}>
          <MasteryBar ratio={progress.ratio} testID="TamilLessonRowBar" />
        </View>
        <Text variant="small" color="$color11" testID="TamilLessonRowCount">
          {progress.mastered}/{progress.total}
        </Text>
      </XStack>
    </ListItem>
  );
}

function HeroChip({
  children,
  testID,
}: {
  children: React.ReactNode;
  testID: string;
}) {
  return (
    <YStack px="$2.5" py="$1" rounded="$10" bg="rgba(255,255,255,0.2)" testID={testID}>
      <Text variant="small" color="white" testID={`${testID}Text`}>
        {children}
      </Text>
    </YStack>
  );
}

export function TamilCourseScreen(): React.ReactElement {
  const insets = useSafeAreaInsets();
  const navigation = useNavigation();
  const characters = useTamilProgressStore((s) => s.characters);

  const groups = useMemo(() => TamilCourse.getLessonGroups(), []);
  const totalCharacters = useMemo(
    () => TamilCourse.getAllCharacters().length,
    []
  );

  // The store keeps CharacterProgress; the domain wants plain rungs. Derived
  // here rather than in the store so the selector stays a stable reference.
  const mastery = useMemo(() => {
    const entries = Object.entries(characters).map(
      ([id, progress]) => [id, progress.mastery] as const
    );
    return Object.fromEntries(entries);
  }, [characters]);

  const overall = useMemo(
    () => lessonProgress(TamilCourse.getAllCharacterIds(), mastery),
    [mastery]
  );

  const openDrawer = useCallback(() => {
    navigation.getParent()?.dispatch({ type: 'OPEN_DRAWER' });
  }, [navigation]);

  const expanded = (
    <YStack bg="$green9" px="$4" pb="$5" pt={insets.top + 12} testID="TamilCourseHero">
      <IconButton
        icon={Menu}
        variant="ghost"
        onPress={openDrawer}
        aria-label="Open menu"
        testID="TamilCourseMenuButton"
      />
      <XStack items="center" gap="$2" mt="$2">
        <HeroChip testID="TamilCourseHeroCount">{totalCharacters} LETTERS</HeroChip>
        <HeroChip testID="TamilCourseHeroMastered">
          {overall.mastered} MASTERED
        </HeroChip>
      </XStack>
      <Text variant="h1" color="white" mt="$3" testID="TamilCourseHeroTitle">
        தமிழ் எழுத்து
      </Text>
      <Text color="white" opacity={0.85} mt="$1" testID="TamilCourseHeroSubtitle">
        Trace every letter until your hand knows it. ✍️
      </Text>
    </YStack>
  );

  const compact = (
    <XStack
      bg="$green9"
      items="center"
      gap="$2"
      px="$4"
      pb="$3"
      pt={insets.top + 6}
      testID="TamilCourseCompactBar"
    >
      <IconButton
        icon={Menu}
        variant="ghost"
        onPress={openDrawer}
        aria-label="Open menu"
        testID="TamilCourseMenuButtonCompact"
      />
      <Text
        variant="large"
        color="white"
        flex={1}
        numberOfLines={1}
        testID="TamilCourseCompactTitle"
      >
        தமிழ் எழுத்து
      </Text>
      <HeroChip testID="TamilCourseCompactCount">
        {overall.mastered}/{totalCharacters}
      </HeroChip>
    </XStack>
  );

  return (
    <View flex={1} bg="$background" testID="TamilCourseScreen">
      <CollapsingHeader
        expanded={expanded}
        compact={compact}
        contentContainerStyle={CONTENT_STYLE}
        testID="TamilCourseHeader"
        scrollViewTestID="TamilCourseScroll"
      >
        <YStack mx="$4" mt="$3" gap="$2" testID="TamilCourseOverall">
          <XStack items="center" justify="space-between">
            <Text variant="strong" testID="TamilCourseOverallLabel">
              Course progress
            </Text>
            <Text variant="muted" testID="TamilCourseOverallValue">
              {Math.round(overall.ratio * 100)}%
            </Text>
          </XStack>
          <MasteryBar ratio={overall.ratio} testID="TamilCourseOverallBar" />
          <Button
            variant="outline"
            testID="TamilCourseTracks"
            pressLog="off"
            onPress={() => {
              Logger.info('tamil.tracks.list_open', {});
              router.push('/tamil/tracks');
            }}
          >
            Write it large · letters and words
          </Button>
          <Button
            variant="default"
            testID="TamilCourseReview"
            pressLog="off"
            onPress={() => {
              Logger.info('tamil.review.open', { scope: 'course' });
              router.push('/tamil/review');
            }}
          >
            Write from memory
          </Button>
        </YStack>

        {groups.map((group) => (
          <Fragment key={group.section}>
            <Text
              variant="h3"
              mt="$5"
              mb="$2"
              mx="$4"
              testID="TamilCourseSectionTitle"
            >
              {group.title}
            </Text>
            <InsetGroup testID="TamilCourseLessons">
              {group.lessons.map((lesson) => (
                <LessonRow
                  key={lesson.id}
                  lesson={lesson}
                  progress={lessonProgress(lesson.characterIds, mastery)}
                />
              ))}
            </InsetGroup>
          </Fragment>
        ))}
      </CollapsingHeader>
    </View>
  );
}
