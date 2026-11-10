import { Logger } from '@lib/logging';
import { Badge, Button, IconButton, ScreenHeader, Text, View } from '@lib/tamagui-design-system';
import { ChevronRight, Eye, RotateCcw, Trash2 } from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import {
  checkWriting,
  WritingCheckError,
  type WritingVerdict,
} from '../data/tamilWritingCheck';
import { strokesToSvg, type CanvasSize, type Stroke } from '../domain/strokes';
import { getWritingSubject, subjectPosition } from '../domain/tracks';
import { useTamilProgressStore } from '../state/tamilProgressStore';

import { TracingCanvas } from './TracingCanvas';

export type TamilWriteScreenProps = {
  /** A character id, or a word id from `domain/tracks.ts`. */
  subjectId: string;
};

/** The printed template's share of the pad's shorter side. */
const TEMPLATE_SCALE = 0.82;
/** A word is long; give it what fits. */
const WORD_TEMPLATE_SCALE = 0.34;
/** The verdict card with its two-line summary, measured in the lab render. */
const VERDICT_HEIGHT = 60;

/**
 * Write the letter -- or the word -- as large as you like, with the shape to
 * copy and the words that describe it. One screen, no scrolling: the header
 * stays put, the instructions take one card, the pad takes everything else,
 * and the controls sit at the bottom under the thumb.
 *
 * Deliberately not the stroke coach next door: no dot to start on, no arrow to
 * follow, no stroke refused for going the wrong way. The template is the font's
 * own glyph, so it is as crisp as the pad is large; the marking happens once,
 * at the end, on what was actually written.
 */
export function TamilWriteScreen({ subjectId }: TamilWriteScreenProps) {
  const insets = useSafeAreaInsets();
  const recordAttempt = useTamilProgressStore((state) => state.recordAttempt);

  const subject = useMemo(() => getWritingSubject(subjectId), [subjectId]);
  const position = useMemo(() => subjectPosition(subjectId), [subjectId]);
  const next = position ? position.track.subjects[position.index + 1] : undefined;

  const [strokes, setStrokes] = useState<readonly Stroke[]>([]);
  const [canvasSize, setCanvasSize] = useState<CanvasSize>({ width: 0, height: 0 });
  const [showTemplate, setShowTemplate] = useState(true);
  const [showSteps, setShowSteps] = useState(true);
  const [verdict, setVerdict] = useState<WritingVerdict | null>(null);
  const [checking, setChecking] = useState(false);

  const clear = useCallback(() => {
    setStrokes([]);
    setVerdict(null);
  }, []);

  const check = useCallback(async () => {
    if (!subject || strokes.length === 0) return;
    setChecking(true);
    setVerdict(null);
    const startedAt = Date.now();
    try {
      const result = await checkWriting({
        characterId: subject.id,
        glyph: subject.glyph,
        transliteration: subject.transliteration,
        svg: strokesToSvg(strokes, canvasSize),
      });
      Logger.info('tamil.write.checked', {
        subjectId: subject.id,
        ok: result.ok,
        score: result.score,
        ms: Date.now() - startedAt,
      });
      setVerdict(result);
      // A word marked right is every one of its letters written right.
      for (const id of subject.characterIds) {
        recordAttempt(id, result.ok ? 'got-it' : 'again');
      }
    } catch (error) {
      // status 0: no response (network or the abort); null: not an HTTP failure (an unreadable 200)
      Logger.withError(error).error('tamil.write.check_failed', {
        subjectId: subject.id,
        status: error instanceof WritingCheckError ? error.status : null,
        ms: Date.now() - startedAt,
      });
      setVerdict({
        ok: false,
        score: 0,
        summary: 'Could not reach the marker. Your writing is still on the pad.',
      });
    } finally {
      setChecking(false);
    }
  }, [subject, canvasSize, recordAttempt, strokes]);

  const missing = !subject;
  useEffect(() => {
    if (missing) Logger.info('tamil.write.not_found', { subjectId });
  }, [missing, subjectId]);

  if (!subject) {
    return (
      <View flex={1} bg="$background" testID="TamilWriteMissing">
        <ScreenHeader
          title="Nothing to write"
          onBack={() => router.back()}
          testID="TamilWriteMissingHeader"
        />
        <YStack p="$4">
          <Text variant="muted" testID="TamilWriteMissingText">
            No letter or word with that id.
          </Text>
        </YStack>
      </View>
    );
  }

  const isWord = subject.kind === 'word';

  return (
    <View flex={1} bg="$background" testID="TamilWriteScreen">
      <ScreenHeader
        title={`${subject.glyph} · ${subject.transliteration}`}
        subtitle={subject.label}
        onBack={() => router.back()}
        testID="TamilWriteHeader"
      />

      <YStack flex={1} px="$3" pt="$2" pb={Math.max(insets.bottom, 12)} gap="$2">
        {showSteps ? (
          <YStack
            bg="$color2"
            rounded="$4"
            px="$3"
            py="$2"
            gap="$1"
            testID="TamilWriteStepsCard"
          >
            {subject.steps.map((step, index) => (
              <XStack key={step} gap="$2" items="flex-start">
                <Text variant="small" color="$color10" testID={`TamilWriteStepNumber${index + 1}`}>
                  {index + 1}.
                </Text>
                <Text flex={1} variant="small" testID={`TamilWriteStep${index + 1}`}>
                  {step}
                </Text>
              </XStack>
            ))}
          </YStack>
        ) : null}

        <TracingCanvas
          glyph={subject.glyph}
          strokes={strokes}
          onStroke={(stroke) => setStrokes((current) => [...current, stroke])}
          onSizeChange={setCanvasSize}
          showGuide={showTemplate}
          height="fill"
          templateScale={isWord ? WORD_TEMPLATE_SCALE : TEMPLATE_SCALE}
          testID="TamilWriteCanvas"
        />

        {/* Room for the verdict from the start: a pad that shrinks under the ink leaves it off the template. */}
        <YStack minH={VERDICT_HEIGHT} justify="center">
          {verdict ? (
            <XStack
              items="center"
              gap="$2"
              bg="$color2"
              rounded="$4"
              px="$3"
              py="$2"
              testID="TamilWriteVerdictCard"
            >
              <Badge variant={verdict.ok ? 'success' : 'warning'} testID="TamilWriteVerdictBadge">
                {verdict.ok ? 'LOOKS RIGHT' : 'NOT YET'} · {verdict.score}
              </Badge>
              <Text flex={1} variant="small" numberOfLines={2} testID="TamilWriteVerdictSummary">
                {verdict.fix ? `${verdict.summary} ${verdict.fix}` : verdict.summary}
              </Text>
            </XStack>
          ) : null}
        </YStack>

        <XStack items="center" gap="$2">
          <Text variant="small" color="$color10" flex={1} testID="TamilWriteHint">
            {strokes.length === 0
              ? 'Write it as large as you like.'
              : `${strokes.length} stroke${strokes.length === 1 ? '' : 's'}`}
          </Text>
          <IconButton
            icon={Eye}
            variant={showTemplate ? 'secondary' : 'outline'}
            aria-label={showTemplate ? 'Hide the template' : 'Show the template'}
            testID="TamilWriteToggleTemplate"
            onPress={() => setShowTemplate((visible) => !visible)}
          />
          <IconButton
            icon={RotateCcw}
            variant="outline"
            aria-label="Undo last stroke"
            testID="TamilWriteUndo"
            onPress={() => setStrokes((current) => current.slice(0, -1))}
          />
          <IconButton
            icon={Trash2}
            variant="outline"
            aria-label="Clear the pad"
            testID="TamilWriteClear"
            onPress={clear}
          />
        </XStack>

        <XStack gap="$2">
          <Button
            flex={1}
            variant="outline"
            testID="TamilWriteToggleSteps"
            pressLog="off"
            onPress={() => setShowSteps((visible) => !visible)}
          >
            {showSteps ? 'Hide steps' : 'Show steps'}
          </Button>
          <Button
            flex={2}
            variant="default"
            testID="TamilWriteCheck"
            pressLog="off"
            disabled={checking || strokes.length === 0}
            onPress={() => {
              void check();
            }}
          >
            {checking ? 'Marking…' : 'Check my writing'}
          </Button>
          {next ? (
            <IconButton
              icon={ChevronRight}
              variant="outline"
              aria-label={`Next: ${next.glyph}`}
              testID="TamilWriteNext"
              onPress={() => {
                Logger.info('tamil.write.next', { from: subject.id, to: next.id });
                router.replace(`/tamil/write/${next.id}`);
              }}
            />
          ) : null}
        </XStack>
      </YStack>
    </View>
  );
}
