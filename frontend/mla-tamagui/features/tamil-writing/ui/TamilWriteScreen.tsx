import { Logger } from '@lib/logging';
import { CompactIconButton, FloatingSurface, ScreenHeader, useCompactColors } from '@lib/tamagui-design-system';
import { ChevronLeft, ChevronRight, Eye, Play, RotateCcw, Trash2 } from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { useEffect, useRef, useState } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { characterPosition } from '../domain/handwriting/catalog';
import { getStrokeGuide } from '../domain/strokeGuide';
import { assessStroke } from '../domain/strokeValidation';
import type { CanvasSize, Stroke } from '../domain/strokes';
import { getWritingSubject, subjectPosition } from '../domain/tracks';
import { useTamilProgressStore } from '../state/tamilProgressStore';
import { TracingCanvas } from './TracingCanvas';
import { backTo, CourseButton, CourseLayout, CourseMessage } from './library/CourseLayout';

const palette = [
  { label: 'Blue', color: '#267ef2' }, { label: 'Red', color: '#eb4f59' }, { label: 'Green', color: '#24a977' },
];

export function TamilWriteScreen({ subjectId }: { subjectId: string }) {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  const recordAttempt = useTamilProgressStore((state) => state.recordAttempt);
  const subject = getWritingSubject(subjectId);
  const position = characterPosition(subjectId);
  const track = subjectPosition(subjectId);
  const previous = position?.previous ?? (position ? undefined : track?.track.subjects[track.index - 1]);
  const next = position?.next ?? (position ? undefined : track?.track.subjects[track.index + 1]);
  const guide = getStrokeGuide(subjectId);
  const [showTemplate, setShowTemplate] = useState(true);
  const [mode, setMode] = useState<'none' | 'guide' | 'demo'>('none');
  const [demoStep, setDemoStep] = useState(0);
  const [width, setWidth] = useState(8);
  const [color, setColor] = useState(palette[0].color);
  const [ink, setInk] = useState<readonly { stroke: Stroke; color: string; width: number }[]>([]);
  const saving = useRef(false);
  const attempt = useRef<string>(undefined);
  const [pending, setPending] = useState(false);
  const [size, setSize] = useState<CanvasSize>({ width: 0, height: 0 });
  const [feedback, setFeedback] = useState<string>();
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (mode !== 'demo' || !guide) return;
    const timer = setTimeout(() => {
      if (demoStep + 1 < guide.strokes.length) setDemoStep(demoStep + 1);
      else setMode('none');
    }, 650);
    return () => clearTimeout(timer);
  }, [demoStep, guide, mode]);

  function open(id: string) {
    router.replace({ pathname: '/tamil/write/[subjectId]', params: { subjectId: id } });
  }

  function reset() {
    attempt.current = undefined;
    setInk([]);
    setFeedback(undefined);
    setSaved(false);
  }

  function check() {
    if (!guide || mode !== 'guide') return;
    if (ink.length !== guide.strokes.length) {
      setFeedback(`Try the ${guide.strokes.length} guide strokes in order.`);
      return;
    }
    const assessment = ink.map((mark, index) => assessStroke(mark.stroke, guide.strokes[index], size)).find((item) => !item.ok);
    setFeedback(assessment?.message ?? 'Your strokes followed the guide.');
  }

  async function finish(outcome: 'got-it' | 'again') {
    if (!subject || !ink.length || saved || saving.current) return;
    saving.current = true;
    setPending(true);
    attempt.current ??= `${subject.id}-${Date.now()}`;
    try {
      if (subject.kind === 'letter') await Promise.resolve(recordAttempt(subject.id, outcome, attempt.current));
      if (outcome === 'again') reset();
      else {
        setSaved(true);
        setFeedback('Practice complete');
      }
    } catch (error) {
      Logger.withError(error).error('tamil.writing.progress_save_failed');
      setFeedback('Couldn’t save your progress. Try again.');
    } finally {
      saving.current = false;
      setPending(false);
    }
  }

  const reference = mode === 'demo' ? demoStep : Math.min(ink.length, (guide?.strokes.length ?? 1) - 1);
  const activeGuide = mode !== 'none' ? guide?.strokes[reference] : undefined;
  const completedGuides = mode !== 'none' ? guide?.strokes.slice(0, reference) : undefined;
  const started = ink.length > 0;
  const title = subject ? `${subject.glyph} · ${subject.transliteration}` : 'Nothing to write';
  const back = () => backTo('/tamil/handwriting');

  return (
    <CourseLayout title={title} onBack={back} testID={subject ? 'TamilWriteScreen' : 'TamilWriteMissing'}
      header={<ScreenHeader glass title={title} subtitle={subject?.label} onBack={back}
        testID={subject ? 'TamilWriteHeader' : 'TamilWriteMissingHeader'} />}>
      {!subject ? <CourseMessage text="No letter or word with that id." /> : (
        <View testID="TamilWriteControlsInset" style={[styles.content, { paddingBottom: Math.max(bottom, 12) }]}>
          <TracingCanvas glyph={subject.glyph} strokes={ink.map((mark) => mark.stroke)} strokePens={ink}
            pen={{ color, width }} onStroke={(stroke) => {
              if (saving.current) return;
              if (mode === 'demo') setMode('none');
              attempt.current = undefined;
              setInk((marks) => [...marks, { stroke, color, width }]);
              setFeedback(undefined);
              setSaved(false);
            }} onSizeChange={setSize} showGuide={showTemplate}
            completedGuides={completedGuides} activeGuide={activeGuide}
            outline={mode !== 'none' ? guide?.contours : undefined}
            templateScale={subject.kind === 'word' ? 0.34 : 0.82} height="fill" testID="TamilWriteCanvas" />
          <FloatingSurface style={styles.controls}>
            <View style={styles.row}>
              <CompactIconButton icon={ChevronLeft} label="Previous character" disabled={!previous}
                onPress={() => { if (previous) open(previous.id); }} />
              {guide && <CompactIconButton icon={Play} label={mode === 'demo' ? 'Stop demo' : 'Play stroke demo'}
                onPress={() => { setDemoStep(0); setMode(mode === 'demo' ? 'none' : 'demo'); }} />}
              <CompactIconButton icon={Eye} label={showTemplate ? 'Hide template' : 'Show template'}
                testID="TamilWriteTemplate" onPress={() => setShowTemplate(!showTemplate)} />
              <CompactIconButton icon={RotateCcw} label="Undo last stroke" disabled={!started}
                testID="TamilWriteUndo" onPress={() => { attempt.current = undefined; setInk((marks) => marks.slice(0, -1)); setFeedback(undefined); setSaved(false); }} />
              <CompactIconButton icon={Trash2} label="Clear ink" disabled={!started} testID="TamilWriteClear" onPress={reset} />
              <CompactIconButton icon={ChevronRight} label="Next character" disabled={!next}
                onPress={() => { if (next) open(next.id); }} />
            </View>
            <View style={styles.row}>
              {palette.map((item) => (
                <Pressable key={item.label} accessibilityRole="button" accessibilityLabel={`${item.label} ink`}
                  accessibilityState={{ selected: item.color === color }} onPress={() => setColor(item.color)}
                  style={[styles.swatch, { backgroundColor: item.color, borderColor: item.color === color ? colors.foreground : colors.raised }]} />
              ))}
              <Pressable accessibilityRole="button" accessibilityLabel={`Pen width ${width}. Tap to change.`}
                onPress={() => setWidth(width === 8 ? 12 : width === 12 ? 4 : 8)} style={styles.option}>
                <Text style={{ color: colors.foreground }}>Pen {width}</Text>
              </Pressable>
              {guide && <Pressable accessibilityRole="button" accessibilityState={{ selected: mode === 'guide' }}
                accessibilityLabel="Toggle stroke guide"
                onPress={() => { setShowTemplate(true); setMode(mode === 'guide' ? 'none' : 'guide'); }} style={styles.option}>
                <Text style={{ color: mode === 'guide' ? colors.gold : colors.foreground }}>Guide</Text>
              </Pressable>}
            </View>
          </FloatingSurface>
          <View style={styles.status}>
            {feedback
              ? <Text testID="TamilWriteVerdict" accessibilityLiveRegion="polite" style={[styles.label, { color: colors.foreground }]}>{feedback}</Text>
              : <Text testID="TamilWriteHint" style={[styles.label, { color: colors.muted }]}>
                  {mode === 'guide' && activeGuide ? activeGuide.hint
                    : started ? `${ink.length} stroke${ink.length === 1 ? '' : 's'}` : 'Write it as large as you like.'}
                </Text>}
          </View>
          <View style={styles.actions}>
            <CourseButton label="Try again" disabled={!started || saved || pending} onPress={() => { void finish('again'); }} />
            {mode === 'guide' && <CourseButton label="Check strokes" disabled={!started} onPress={check} />}
            <CourseButton label="Done" testID="TamilWriteDone" disabled={!started || saved || pending} onPress={() => { void finish('got-it'); }} />
          </View>
        </View>
      )}
    </CourseLayout>
  );
}

const styles = StyleSheet.create({
  content: { flex: 1, paddingHorizontal: 14, gap: 8 },
  controls: { borderRadius: 21, padding: 5, gap: 4 },
  row: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-evenly' },
  swatch: { height: 32, width: 32, borderRadius: 16, borderWidth: 2 },
  option: { minHeight: 44, justifyContent: 'center', paddingHorizontal: 6 },
  label: { fontSize: 13, textAlign: 'center' },
  status: { height: 34, justifyContent: 'center' },
  actions: { flexDirection: 'row', justifyContent: 'center', gap: 7 },
});
