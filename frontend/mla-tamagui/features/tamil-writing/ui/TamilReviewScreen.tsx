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
import { Eye, RotateCcw, Trash2 } from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { ScrollView, XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { TamilCourse } from '../domain/course';
import type { TamilCharacter } from '../domain/characters';
import {
  answer,
  counts,
  createDeck,
  currentCard,
  type ReviewDeck,
} from '../domain/reviewDeck';
import { getStrokeGuide } from '../domain/strokeGuide';
import { FIT_MESSAGES, describeFit, type CanvasSize, type Stroke } from '../domain/strokes';
import { useTamilProgressStore } from '../state/tamilProgressStore';

import { MasteryBar } from './MasteryBar';
import { TracingCanvas } from './TracingCanvas';

export type TamilReviewScreenProps = {
  /** Restrict the deck to one lesson. Omitted, it draws on the whole course. */
  lessonId?: string;
};

/**
 * Write the letter from its name, then say whether you got it.
 *
 * The prompt is the transliteration and the sound, never the letter: a card
 * that shows what you are meant to recall is not a card. The printed letter is
 * one tap away for when it will not come, and revealing it is not held against
 * the learner -- the pile they put it in is.
 *
 * Marking is the learner's own. Nothing here can tell a passable அ from a good
 * one, and pretending otherwise would teach the wrong thing; what the app does
 * is keep the two piles honest, and bring the missed letters round again
 * sooner than the rest.
 */
export function TamilReviewScreen({ lessonId }: TamilReviewScreenProps = {}) {
  const masteryMap = useTamilProgressStore((state) => state.masteryMap);
  const recordAttempt = useTamilProgressStore((state) => state.recordAttempt);

  const characters = useMemo<readonly TamilCharacter[]>(() => {
    if (!lessonId) return TamilCourse.getAllCharacters();
    return TamilCourse.getLessonCharacters(lessonId);
  }, [lessonId]);

  const byId = useMemo(() => {
    const map = new Map<string, TamilCharacter>();
    for (const character of characters) map.set(character.id, character);
    return map;
  }, [characters]);

  const [deck, setDeck] = useState<ReviewDeck>(() =>
    createDeck(
      characters.map((character) => character.id),
      masteryMap()
    )
  );
  const [strokes, setStrokes] = useState<readonly Stroke[]>([]);
  const [canvasSize, setCanvasSize] = useState<CanvasSize>({ width: 0, height: 0 });
  const [revealed, setRevealed] = useState(false);

  const cardId = currentCard(deck);
  const character = cardId ? byId.get(cardId) : undefined;
  const guide = character ? getStrokeGuide(character.id) : undefined;
  const tally = counts(deck);

  const fit = describeFit(strokes, canvasSize);

  const clear = useCallback(() => {
    setStrokes([]);
  }, []);

  const mark = useCallback(
    (outcome: 'got-it' | 'again') => {
      if (!character) return;
      Logger.info('tamil.review.mark', { characterId: character.id, outcome });
      recordAttempt(character.id, outcome);
      setDeck((current) => answer(current, outcome));
      setStrokes([]);
      setRevealed(false);
    },
    [character, recordAttempt]
  );

  // Answered cards are re-queued, never dropped: the deck is empty only for a lesson id with no letters.
  const empty = !character;
  useEffect(() => {
    if (empty) Logger.info('tamil.review.empty', { lessonId: lessonId ?? null });
  }, [empty, lessonId]);

  if (!character) {
    return (
      <View flex={1} bg="$background" testID="TamilReviewEmpty">
        <ScreenHeader
          title="Nothing to review"
          onBack={() => router.back()}
          testID="TamilReviewEmptyHeader"
        />
        <YStack p="$4">
          <Text variant="muted" testID="TamilReviewEmptyText">
            This deck has no letters in it.
          </Text>
        </YStack>
      </View>
    );
  }

  const reviewed = tally.known + tally.learning;

  return (
    <View flex={1} bg="$background" testID="TamilReviewScreen">
      <ScreenHeader
        title="Write from memory"
        subtitle={lessonId ? 'This lesson' : 'Every letter'}
        onBack={() => router.back()}
        testID="TamilReviewHeader"
      />

      <ScrollView flex={1} testID="TamilReviewScroll">
        <YStack px="$4" pt="$3" pb="$6" gap="$3">
          <XStack items="center" justify="space-between" gap="$2">
            <Badge variant="success" testID="TamilReviewKnownCount">
              {tally.known} GOT IT
            </Badge>
            <Badge variant="warning" testID="TamilReviewLearningCount">
              {tally.learning} AGAIN
            </Badge>
            <Badge variant="outline" testID="TamilReviewRemainingCount">
              {tally.unseen} TO GO
            </Badge>
          </XStack>

          <MasteryBar
            ratio={characters.length > 0 ? reviewed / characters.length : 0}
            testID="TamilReviewBar"
          />

          <Card testID="TamilReviewPromptCard">
            <CardContent>
              <YStack gap="$1">
                <Text variant="muted">Write this letter</Text>
                <Text variant="h2" testID="TamilReviewPrompt">
                  {character.transliteration}
                </Text>
                <Text variant="small" testID="TamilReviewSound">
                  {character.sound}
                </Text>
                {character.example ? (
                  <Text variant="small" testID="TamilReviewExample">
                    {revealed
                      ? `${character.example.word} · ${character.example.transliteration} · ${character.example.meaning}`
                      : `as in ${character.example.transliteration} — ${character.example.meaning}`}
                  </Text>
                ) : null}
              </YStack>
            </CardContent>
          </Card>

          <TracingCanvas
            glyph={character.glyph}
            strokes={strokes}
            onStroke={(stroke) => setStrokes((current) => [...current, stroke])}
            onSizeChange={setCanvasSize}
            showGuide={revealed}
            {...(revealed && guide ? { outline: guide.contours } : {})}
            testID="TamilReviewCanvas"
          />

          <XStack items="center" justify="space-between" gap="$2">
            <Text variant="muted" flex={1} testID="TamilReviewFeedback">
              {strokes.length > 0 ? FIT_MESSAGES[fit] : 'Write it with your finger.'}
            </Text>
            <XStack gap="$2">
              <IconButton
                icon={Eye}
                variant={revealed ? 'secondary' : 'outline'}
                aria-label={revealed ? 'Hide the letter' : 'Show the letter'}
                testID="TamilReviewReveal"
                onPress={() => setRevealed((visible) => !visible)}
              />
              <IconButton
                icon={Trash2}
                variant="outline"
                aria-label="Clear the pad"
                testID="TamilReviewClear"
                onPress={clear}
              />
              <IconButton
                icon={RotateCcw}
                variant="outline"
                aria-label="Undo last stroke"
                testID="TamilReviewUndo"
                onPress={() => setStrokes((current) => current.slice(0, -1))}
              />
            </XStack>
          </XStack>

          {revealed ? (
            <Card testID="TamilReviewAnswerCard">
              <CardContent>
                <XStack items="center" gap="$3">
                  <Text fontSize={56} lineHeight={72} testID="TamilReviewAnswerGlyph">
                    {character.glyph}
                  </Text>
                  <YStack flex={1} gap="$1">
                    <Text variant="h3" testID="TamilReviewAnswerName">
                      {character.tamilName}
                    </Text>
                    <Text variant="muted" testID="TamilReviewAnswerHint">
                      {character.strokeHint}
                    </Text>
                  </YStack>
                </XStack>
              </CardContent>
            </Card>
          ) : null}

          <XStack gap="$3">
            <Button
              flex={1}
              variant="outline"
              testID="TamilReviewAgain"
              pressLog="off"
              onPress={() => mark('again')}
            >
              Not yet
            </Button>
            <Button
              flex={1}
              variant="default"
              testID="TamilReviewGotIt"
              pressLog="off"
              onPress={() => mark('got-it')}
            >
              Got it
            </Button>
          </XStack>

          <Text variant="muted" testID="TamilReviewAnsweredCount">
            {tally.answered} answered · missed letters come back sooner
          </Text>
        </YStack>
      </ScrollView>
    </View>
  );
}
