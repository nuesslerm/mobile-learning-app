import { Logger } from '@lib/logging';
import {
  Badge,
  Button,
  Card,
  CardContent,
  IconButton,
  ScreenHeader,
  Text,
  View,
} from '@lib/tamagui-design-system';
import {
  ChevronLeft,
  ChevronRight,
  Eye,
  RotateCcw,
  Trash2,
} from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { ScrollView, XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { TamilCourse } from '../domain/course';
import {
  MASTERY_MASTERED,
  isMastered,
  lessonProgress,
  nextCharacterIndex,
  type PracticeOutcome,
} from '../domain/mastery';
import { getStrokeGuide } from '../domain/strokeGuide';
import {
  assessStroke,
  type StrokeAssessment,
} from '../domain/strokeValidation';
import {
  FIT_MESSAGES,
  describeFit,
  type CanvasSize,
  type Stroke,
} from '../domain/strokes';
import { useTamilProgressStore } from '../state/tamilProgressStore';

import { MasteryBar } from './MasteryBar';
import { TracingCanvas } from './TracingCanvas';

export type TamilLessonScreenProps = {
  lessonId: string;
};

function MasteryDots({ level }: { level: number }) {
  return (
    <XStack gap="$1" items="center" testID="TamilPracticeMasteryDots">
      {Array.from({ length: MASTERY_MASTERED }, (_, i) => (
        <View
          key={i}
          testID={`TamilPracticeMasteryDot${i}`}
          width={8}
          height={8}
          rounded={9999}
          bg={i < level ? '$green9' : '$color5'}
        />
      ))}
    </XStack>
  );
}

export function TamilLessonScreen({
  lessonId,
}: TamilLessonScreenProps): React.ReactElement {
  const lesson = useMemo(() => TamilCourse.getLesson(lessonId), [lessonId]);
  const characters = useMemo(
    () => TamilCourse.getLessonCharacters(lessonId),
    [lessonId]
  );

  const recordAttempt = useTamilProgressStore((s) => s.recordAttempt);
  const resetLesson = useTamilProgressStore((s) => s.resetLesson);
  const progressByCharacter = useTamilProgressStore((s) => s.characters);

  const [index, setIndex] = useState(0);
  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const [strokeIndex, setStrokeIndex] = useState(0);
  const [assessment, setAssessment] = useState<StrokeAssessment | null>(null);
  const [scores, setScores] = useState<number[]>([]);
  const [canvasSize, setCanvasSize] = useState<CanvasSize>({
    width: 0,
    height: 0,
  });
  const [showGlyph, setShowGlyph] = useState(true);

  const character = characters[index];
  const mastery = character
    ? (progressByCharacter[character.id]?.mastery ?? 0)
    : 0;

  // Letters with authored stroke data are written stroke by stroke and marked;
  // the rest fall back to free tracing rather than being scored against a
  // shape nobody authored.
  const guide = useMemo(
    () => (character ? getStrokeGuide(character.id) : undefined),
    [character]
  );
  const activeStroke = guide?.strokes[strokeIndex];
  const done = guide ? strokeIndex >= guide.strokes.length : false;

  const fit = useMemo(
    () => describeFit(strokes, canvasSize),
    [strokes, canvasSize]
  );

  // The bar between the arrows tracks the lesson's mastery, not the current
  // position -- "Letter n of m" above already says where you are.
  const lessonRatio = useMemo(() => {
    const mastered = Object.fromEntries(
      characters.map((c) => [c.id, progressByCharacter[c.id]?.mastery ?? 0])
    );
    return lessonProgress(
      characters.map((c) => c.id),
      mastered
    ).ratio;
  }, [characters, progressByCharacter]);

  const resetPad = useCallback(() => {
    setStrokes([]);
    setStrokeIndex(0);
    setAssessment(null);
    setScores([]);
  }, []);

  const goTo = useCallback(
    (nextIndex: number) => {
      setIndex(nextIndex);
      resetPad();
    },
    [resetPad]
  );

  const handleStroke = useCallback(
    (stroke: Stroke) => {
      if (!guide) {
        setStrokes((current) => [...current, stroke]);
        return;
      }
      const reference = guide.strokes[strokeIndex];
      if (!reference) return;

      const result = assessStroke(stroke, reference, canvasSize);
      setAssessment(result);
      Logger.info('tamil.practice.stroke', {
        characterId: character?.id ?? null,
        strokeIndex,
        ok: result.ok,
        reason: result.reason ?? null,
        score: Math.round(result.score * 100),
      });
      // A refused stroke leaves no ink: the pad should always show the letter
      // as it has actually been written so far.
      if (!result.ok) return;

      setStrokes((current) => [...current, stroke]);
      setScores((current) => [...current, result.score]);
      setStrokeIndex((current) => current + 1);
    },
    [canvasSize, character, guide, strokeIndex]
  );

  const grade = useCallback(
    (outcome: PracticeOutcome) => {
      if (!character) return;
      recordAttempt(character.id, outcome);

      const masteryMap = Object.fromEntries(
        Object.entries(useTamilProgressStore.getState().characters).map(
          ([id, progress]) => [id, progress.mastery] as const
        )
      );
      const ids = characters.map((c) => c.id);
      goTo(nextCharacterIndex(ids, masteryMap, index));
    },
    [character, characters, goTo, index, recordAttempt]
  );

  const undo = useCallback(() => {
    setStrokes((current) => current.slice(0, -1));
    setAssessment(null);
    if (guide) {
      setStrokeIndex((current) => Math.max(0, current - 1));
      setScores((current) => current.slice(0, -1));
    }
  }, [guide]);

  const missing = !lesson || !character;
  useEffect(() => {
    if (missing) Logger.info('tamil.lesson.not_found', { lessonId });
  }, [missing, lessonId]);

  if (!lesson || !character) {
    return (
      <View flex={1} bg="$background" testID="TamilLessonMissing">
        <ScreenHeader
          title="Lesson not found"
          onBack={() => router.back()}
          testID="TamilLessonMissingHeader"
        />
        <YStack p="$4">
          <Text variant="muted" testID="TamilLessonMissingText">
            No lesson with that id.
          </Text>
        </YStack>
      </View>
    );
  }

  const averageScore =
    scores.length > 0
      ? Math.round((scores.reduce((a, b) => a + b, 0) / scores.length) * 100)
      : 0;

  const feedback = assessment?.message ?? (guide ? null : FIT_MESSAGES[fit]);

  return (
    <View flex={1} bg="$background" testID="TamilLessonScreen">
      <ScreenHeader
        title={lesson.title}
        subtitle={lesson.subtitle}
        onBack={() => router.back()}
        actions={[
          {
            icon: RotateCcw,
            accessibilityLabel: 'Reset this lesson',
            testID: 'TamilLessonReset',
            onPress: () => {
              Logger.info('tamil.lesson.reset', { lessonId });
              resetLesson(lesson.characterIds);
              goTo(0);
            },
          },
        ]}
        testID="TamilLessonHeader"
      />

      <ScrollView flex={1} testID="TamilLessonScroll">
        <YStack px="$4" pt="$3" pb="$6" gap="$3">
          <XStack items="center" justify="space-between">
            <Text variant="muted" testID="TamilPracticePosition">
              Letter {index + 1} of {characters.length}
            </Text>
            <XStack items="center" gap="$2">
              <MasteryDots level={mastery} />
              {isMastered(mastery) ? (
                <Badge variant="success" testID="TamilPracticeMasteredBadge">
                  MASTERED
                </Badge>
              ) : null}
            </XStack>
          </XStack>

          <XStack items="center" gap="$3">
            <IconButton
              icon={ChevronLeft}
              variant="ghost"
              aria-label="Previous letter"
              testID="TamilPracticePrevious"
              onPress={() =>
                goTo((index - 1 + characters.length) % characters.length)
              }
            />
            <View flex={1}>
              <MasteryBar ratio={lessonRatio} testID="TamilPracticeLessonBar" />
            </View>
            <IconButton
              icon={ChevronRight}
              variant="ghost"
              aria-label="Next letter"
              testID="TamilPracticeNext"
              onPress={() => goTo((index + 1) % characters.length)}
            />
          </XStack>

          <Card testID="TamilPracticeCharacterCard">
            <CardContent>
              <XStack items="center" gap="$4">
                <Text fontSize={56} lineHeight={72} testID="TamilPracticeGlyph">
                  {character.glyph}
                </Text>
                <YStack flex={1} gap="$1">
                  <Text variant="h3" testID="TamilPracticeTransliteration">
                    {character.transliteration}
                  </Text>
                  <Text variant="muted" testID="TamilPracticeTamilName">
                    {character.tamilName}
                  </Text>
                  <Text variant="small" testID="TamilPracticeSound">
                    {character.sound}
                  </Text>
                </YStack>
              </XStack>
              <Text variant="muted" testID="TamilPracticeStrokeHint">
                {character.strokeHint}
              </Text>
              {character.example ? (
                <Text variant="small" testID="TamilPracticeExample">
                  {character.example.word} · {character.example.transliteration} ·{' '}
                  {character.example.meaning}
                </Text>
              ) : null}
            </CardContent>
          </Card>

          {guide ? (
            <Card
              tone={done ? 'accent' : 'default'}
              testID="TamilPracticeStrokeCoach"
            >
              <XStack items="center" justify="space-between" gap="$3">
                <Text variant="strong" testID="TamilPracticeStrokeStep">
                  {done
                    ? `All ${guide.strokes.length} stroke${guide.strokes.length === 1 ? '' : 's'} written`
                    : `Stroke ${strokeIndex + 1} of ${guide.strokes.length}`}
                </Text>
                {done ? (
                  <Badge variant="success" testID="TamilPracticeStrokeScore">
                    {averageScore}% ON THE LINE
                  </Badge>
                ) : null}
              </XStack>
              <Text variant="muted" testID="TamilPracticeStrokeGuideHint">
                {done
                  ? 'Press Got it, or clear the pad to write it again.'
                  : (activeStroke?.hint ?? '')}
              </Text>
            </Card>
          ) : null}

          <TracingCanvas
            glyph={character.glyph}
            strokes={strokes}
            onStroke={handleStroke}
            onSizeChange={setCanvasSize}
            showGuide={showGlyph}
            {...(guide
              ? {
                  completedGuides: guide.strokes.slice(0, strokeIndex),
                  outline: guide.contours,
                }
              : {})}
            {...(activeStroke ? { activeGuide: activeStroke } : {})}
            testID="TamilPracticeCanvas"
          />

          <XStack items="center" justify="space-between" gap="$2">
            <Text
              variant="muted"
              flex={1}
              color={assessment && !assessment.ok ? '$red10' : '$color11'}
              testID="TamilPracticeFeedback"
            >
              {feedback ?? ''}
            </Text>
            <XStack gap="$2">
              <IconButton
                icon={Eye}
                variant={showGlyph ? 'secondary' : 'outline'}
                aria-label={
                  showGlyph ? 'Hide the printed letter' : 'Show the printed letter'
                }
                testID="TamilPracticeToggleGuide"
                onPress={() => setShowGlyph((visible) => !visible)}
              />
              <IconButton
                icon={RotateCcw}
                variant="outline"
                aria-label="Undo last stroke"
                testID="TamilPracticeUndo"
                onPress={undo}
              />
              <IconButton
                icon={Trash2}
                variant="outline"
                aria-label="Clear the pad"
                testID="TamilPracticeClear"
                onPress={resetPad}
              />
            </XStack>
          </XStack>

          <Button
            variant="outline"
            testID="TamilPracticeWriteBig"
            pressLog="off"
            onPress={() => {
              Logger.info('tamil.write.open', { characterId: character.id });
              router.push(`/tamil/write/${character.id}`);
            }}
          >
            Write it large, with instructions
          </Button>

          <XStack gap="$3">
            <Button
              flex={1}
              variant="outline"
              testID="TamilPracticeAgain"
              pressLog="off"
              onPress={() => grade('again')}
            >
              Practise again
            </Button>
            <Button
              flex={1}
              variant="default"
              testID="TamilPracticeGotIt"
              pressLog="off"
              onPress={() => grade('got-it')}
            >
              Got it
            </Button>
          </XStack>

          <Text variant="muted" testID="TamilLessonDescription">
            {lesson.description}
          </Text>
        </YStack>
      </ScrollView>
    </View>
  );
}
