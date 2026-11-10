/**
 * Screenshot history + comparison viewer for one golden test case, opened by
 * tapping a row in `GoldenScreenshotsList` (see `ServiceReportScreen.tsx`).
 * Modeled on GitHub's PR image-diff UX: a run picker across historical
 * captures of the same case, three comparison modes (Swipe — drag a divider
 * to reveal baseline vs. actual, matching GitHub's "Swipe" mode and this
 * feature's "onion style left/right swipe" ask; Overlay — a transparent
 * blend with a draggable opacity slider, GitHub's "Onion Skin"; Side by
 * side — two images), and a comment thread scoped to the selected capture.
 *
 * Comparison dragging uses plain `PanResponder` + component state rather than
 * Reanimated — this is a low-frequency, non-hot-path gesture (a single
 * modal's compare slider, not a scroll/list interaction), so the extra
 * worklet/shared-value machinery would add ceremony without a real benefit.
 */

import { Logger } from '@lib/logging';
import {
  buildScreenshotKey,
  type GoldenScreenshotHistoryEntry,
} from '../contracts';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Badge,
  Button,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Text,
  Textarea,
  View,
  useOpenSheetStore,
} from '@lib/tamagui-design-system';
import * as React from 'react';
import {
  Image,
  type ImageErrorEventData,
  type NativeSyntheticEvent,
  PanResponder,
  Pressable,
  ScrollView,
} from 'react-native';

import { useGoldenScreenshotComments } from '../state/useGoldenScreenshotComments';

const COMPARE_HEIGHT = 320;

export type GoldenScreenshotViewerState = {
  pkgName: string;
  suite: string;
  name: string;
} | null;

type CompareMode = 'swipe' | 'overlay' | 'side-by-side';

/**
 * A horizontal drag value in `[0, 1]`, driven by touch delta from the pixel
 * position the gesture started at (not absolute screen position) — so the
 * caller never needs the container's page offset, only its width.
 */
function useDragSlider(
  widthRef: React.MutableRefObject<number>,
  initial: number
) {
  const [value, setValue] = React.useState(initial);
  const valueRef = React.useRef(value);
  valueRef.current = value;
  const startPixelRef = React.useRef(0);

  const panResponder = React.useRef(
    PanResponder.create({
      onStartShouldSetPanResponder: () => true,
      onMoveShouldSetPanResponder: () => true,
      onPanResponderGrant: () => {
        startPixelRef.current = valueRef.current * widthRef.current;
      },
      onPanResponderMove: (_evt, gestureState) => {
        const w = widthRef.current;
        if (w <= 0) return;
        const nextPixel = Math.min(
          w,
          Math.max(0, startPixelRef.current + gestureState.dx)
        );
        setValue(nextPixel / w);
      },
    })
  ).current;

  return { value, panResponder };
}

function ImageCompareArea({
  mode,
  baselineUri,
  actualUri,
}: {
  mode: CompareMode;
  baselineUri?: string;
  actualUri?: string;
}) {
  const widthRef = React.useRef(0);
  const [, forceLayoutUpdate] = React.useState(0);
  const { value, panResponder } = useDragSlider(widthRef, 0.5);

  if (!baselineUri && !actualUri) {
    return (
      <Text variant="muted" testID="GoldenScreenshotViewer-Compare-Empty">
        No images captured for this run.
      </Text>
    );
  }

  const imageFailed =
    (side: 'baseline' | 'actual') =>
    (e: NativeSyntheticEvent<ImageErrorEventData>) => {
      Logger.warn('service_report.golden_image_failed', {
        side,
        mode,
        error: e.nativeEvent.error,
      });
    };

  if (mode === 'side-by-side') {
    return (
      <View
        gap="$2"
        style={{ height: COMPARE_HEIGHT }}
        testID="GoldenScreenshotViewer-Compare-SideBySide"
      >
        <View
          gap="$1"
          testID="GoldenScreenshotViewer-Compare-SideBySide-Baseline"
        >
          <Text
            variant="muted"
            testID="GoldenScreenshotViewer-Compare-SideBySide-BaselineLabel"
          >
            Baseline
          </Text>
          {baselineUri ? (
            <Image
              source={{ uri: baselineUri }}
              resizeMode="contain"
              style={{ flex: 1 }}
              onError={imageFailed('baseline')}
              testID="GoldenScreenshotViewer-Compare-Baseline"
            />
          ) : (
            <Text
              variant="muted"
              testID="GoldenScreenshotViewer-Compare-SideBySide-BaselineNone"
            >
              (none)
            </Text>
          )}
        </View>
        <View
          gap="$1"
          testID="GoldenScreenshotViewer-Compare-SideBySide-ActualBox"
        >
          <Text
            variant="muted"
            testID="GoldenScreenshotViewer-Compare-SideBySide-ActualLabel"
          >
            Actual
          </Text>
          {actualUri ? (
            <Image
              source={{ uri: actualUri }}
              resizeMode="contain"
              style={{ flex: 1 }}
              onError={imageFailed('actual')}
              testID="GoldenScreenshotViewer-Compare-Actual"
            />
          ) : (
            <Text
              variant="muted"
              testID="GoldenScreenshotViewer-Compare-SideBySide-ActualNone"
            >
              (none)
            </Text>
          )}
        </View>
      </View>
    );
  }

  return (
    <View
      gap="$2"
      testID={
        mode === 'swipe'
          ? 'GoldenScreenshotViewer-Compare-Swipe'
          : 'GoldenScreenshotViewer-Compare-Overlay'
      }
    >
      <View
        style={{ height: COMPARE_HEIGHT }}
        onLayout={(e) => {
          widthRef.current = e.nativeEvent.layout.width;
          forceLayoutUpdate((n) => n + 1);
        }}
        testID="GoldenScreenshotViewer-Compare-Viewport"
      >
        {baselineUri ? (
          <Image
            source={{ uri: baselineUri }}
            resizeMode="contain"
            style={{
              position: 'absolute',
              left: 0,
              top: 0,
              height: '100%',
              width: '100%',
            }}
            onError={imageFailed('baseline')}
            testID="GoldenScreenshotViewer-Compare-Baseline"
          />
        ) : null}
        {actualUri && mode === 'swipe' ? (
          <View
            style={{ width: widthRef.current * value }}
            testID="GoldenScreenshotViewer-Compare-SwipeClip"
          >
            <Image
              source={{ uri: actualUri }}
              resizeMode="contain"
              style={{ width: widthRef.current, height: COMPARE_HEIGHT }}
              onError={imageFailed('actual')}
              testID="GoldenScreenshotViewer-Compare-Actual"
            />
          </View>
        ) : null}
        {actualUri && mode === 'overlay' ? (
          <Image
            source={{ uri: actualUri }}
            resizeMode="contain"
            style={{
              position: 'absolute',
              left: 0,
              top: 0,
              height: '100%',
              width: '100%',
              opacity: value,
            }}
            onError={imageFailed('actual')}
            testID="GoldenScreenshotViewer-Compare-Actual"
          />
        ) : null}
        {mode === 'swipe' ? (
          <View
            {...panResponder.panHandlers}
            style={{ left: Math.max(0, widthRef.current * value - 12) }}
            testID="GoldenScreenshotViewer-Compare-Handle"
          >
            <View testID="GoldenScreenshotViewer-Compare-HandleBar" />
          </View>
        ) : null}
      </View>
      {mode === 'overlay' ? (
        <View
          {...panResponder.panHandlers}
          testID="GoldenScreenshotViewer-Compare-OpacitySlider"
        >
          <View testID="GoldenScreenshotViewer-Compare-OpacitySlider-Track">
            <View
              style={{ width: `${value * 100}%` }}
              testID="GoldenScreenshotViewer-Compare-OpacitySlider-Fill"
            />
          </View>
        </View>
      ) : null}
      <Text variant="muted" testID="GoldenScreenshotViewer-Compare-Caption">
        {mode === 'swipe'
          ? 'Drag the divider to reveal baseline vs. actual'
          : `Actual opacity ${Math.round(value * 100)}% — drag to blend`}
      </Text>
    </View>
  );
}

function RunSelector({
  history,
  selectedIndex,
  onSelect,
}: {
  history: GoldenScreenshotHistoryEntry[];
  selectedIndex: number;
  onSelect: (index: number) => void;
}) {
  return (
    <ScrollView
      horizontal
      showsHorizontalScrollIndicator={false}
      testID="GoldenScreenshotViewer-RunSelector"
    >
      <View gap="$2" testID="GoldenScreenshotViewer-RunSelector-Row">
        {history.map((entry, index) => (
          <Pressable
            key={entry.generatedAt}
            onPress={() => {
              onSelect(index);
            }}
            testID={`GoldenScreenshotViewer-RunSelector-Run-${index}`}
          >
            <Badge
              variant={index === selectedIndex ? 'default' : 'outline'}
              testID={`GoldenScreenshotViewer-RunSelector-Run-${index}-Badge`}
            >
              {entry.generatedAt}
            </Badge>
          </Pressable>
        ))}
      </View>
    </ScrollView>
  );
}

function CommentsSection({
  screenshotKey,
}: {
  screenshotKey: string | undefined;
}) {
  const comments = useGoldenScreenshotComments(screenshotKey);
  const [author, setAuthor] = React.useState('');
  const [commentText, setCommentText] = React.useState('');
  const [posting, setPosting] = React.useState(false);
  const [postError, setPostError] = React.useState<string | undefined>();

  const submit = React.useCallback(() => {
    const trimmed = commentText.trim();
    if (!trimmed) return;
    setPosting(true);
    setPostError(undefined);
    comments
      .post({ author: author.trim() || undefined, commentText: trimmed })
      .then((result) => {
        setPosting(false);
        if (result.ok) {
          setCommentText('');
          Logger.info('service_report.comment_posted');
        } else {
          setPostError(result.error);
        }
      })
      .catch((err: unknown) => {
        setPosting(false);
        Logger.error('service_report.comment_post_unexpected_error', { err });
        setPostError(err instanceof Error ? err.message : String(err));
      });
  }, [author, commentText, comments]);

  return (
    <View gap="$2" testID="GoldenScreenshotViewer-Comments">
      <Text variant="strong" testID="GoldenScreenshotViewer-Comments-Title">
        Comments
      </Text>

      {comments.status === 'loading' ? (
        <Text variant="muted" testID="GoldenScreenshotViewer-Comments-Loading">
          Loading comments…
        </Text>
      ) : comments.status === 'not-configured' ? (
        <Text
          variant="muted"
          testID="GoldenScreenshotViewer-Comments-NotConfigured"
        >
          {comments.reason}
        </Text>
      ) : comments.status === 'unreachable' ? (
        <Text
          variant="muted"
          testID="GoldenScreenshotViewer-Comments-Unreachable"
        >
          {comments.error}
        </Text>
      ) : comments.comments.length === 0 ? (
        <Text variant="muted" testID="GoldenScreenshotViewer-Comments-Empty">
          No comments yet.
        </Text>
      ) : (
        <View gap="$2" testID="GoldenScreenshotViewer-Comments-List">
          {comments.comments.map((comment) => (
            <View
              key={comment.id}
              gap="$0.5"
              testID={`GoldenScreenshotViewer-Comments-Item-${comment.id}`}
            >
              <View
                gap="$2"
                testID={`GoldenScreenshotViewer-Comments-Item-${comment.id}-Header`}
              >
                <Text
                  variant="strong"
                  testID={`GoldenScreenshotViewer-Comments-Item-${comment.id}-Author`}
                >
                  {comment.author || 'anonymous'}
                </Text>
                <Text
                  variant="muted"
                  testID={`GoldenScreenshotViewer-Comments-Item-${comment.id}-CreatedAt`}
                >
                  {comment.createdAt}
                </Text>
              </View>
              <Text
                testID={`GoldenScreenshotViewer-Comments-Item-${comment.id}-Text`}
              >
                {comment.commentText}
              </Text>
            </View>
          ))}
        </View>
      )}

      {comments.status === 'not-configured' ? null : (
        <View gap="$2" testID="GoldenScreenshotViewer-Comments-Form">
          <Textarea
            testID="GoldenScreenshotViewer-Comments-AuthorInput"
            placeholder="Your name (optional)"
            value={author}
            onChangeText={setAuthor}
            numberOfLines={1}
          />
          <Textarea
            testID="GoldenScreenshotViewer-Comments-BodyInput"
            placeholder="Add a comment…"
            value={commentText}
            onChangeText={setCommentText}
          />
          {postError ? (
            <Text
              variant="muted"
              testID="GoldenScreenshotViewer-Comments-PostError"
            >
              {postError}
            </Text>
          ) : null}
          <Button
            onPress={submit}
            disabled={posting || commentText.trim().length === 0}
            testID="GoldenScreenshotViewer-Comments-PostButton"
          >
            {posting ? 'Posting…' : 'Post comment'}
          </Button>
        </View>
      )}
    </View>
  );
}

function GoldenScreenshotViewer({
  state,
  history,
  onClose,
}: {
  state: GoldenScreenshotViewerState;
  history: GoldenScreenshotHistoryEntry[];
  onClose: () => void;
}) {
  const [selectedIndex, setSelectedIndex] = React.useState(0);
  const [mode, setMode] = React.useState<CompareMode>('swipe');
  const { open, close } = useOpenSheetStore();

  React.useEffect(() => {
    if (state !== null) {
      open();
    } else {
      close();
    }
  }, [state, open, close]);

  // Reset to the latest run and default mode each time a different case opens.
  React.useEffect(() => {
    setSelectedIndex(0);
    setMode('swipe');
  }, [state?.pkgName, state?.suite, state?.name]);

  const selected = history[selectedIndex];
  const screenshotKey =
    state && selected
      ? buildScreenshotKey(state.suite, state.name, selected.generatedAt)
      : undefined;

  return (
    <AlertDialog
      open={state !== null}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <AlertDialogContent testID="GoldenScreenshotViewer-Content">
        <AlertDialogHeader>
          <AlertDialogTitle testID="GoldenScreenshotViewer-Title">
            {state ? `${state.suite} — ${state.name}` : ''}
          </AlertDialogTitle>
        </AlertDialogHeader>

        {history.length === 0 ? (
          <Text variant="muted" testID="GoldenScreenshotViewer-Empty">
            No captured runs for this test case yet.
          </Text>
        ) : (
          <ScrollView
            style={{ maxHeight: 520 }}
            testID="GoldenScreenshotViewer-Scroll"
          >
            <View gap="$3" testID="GoldenScreenshotViewer-Body">
              <RunSelector
                history={history}
                selectedIndex={selectedIndex}
                onSelect={setSelectedIndex}
              />

              <Tabs
                value={mode}
                onValueChange={(v) => {
                  setMode(v as CompareMode);
                }}
              >
                <TabsList testID="GoldenScreenshotViewer-Tabs">
                  <TabsTrigger
                    value="swipe"
                    testID="GoldenScreenshotViewer-Tab-Swipe"
                  >
                    Swipe
                  </TabsTrigger>
                  <TabsTrigger
                    value="overlay"
                    testID="GoldenScreenshotViewer-Tab-Overlay"
                  >
                    Overlay
                  </TabsTrigger>
                  <TabsTrigger
                    value="side-by-side"
                    testID="GoldenScreenshotViewer-Tab-SideBySide"
                  >
                    Side by side
                  </TabsTrigger>
                </TabsList>
                <TabsContent value="swipe">
                  <ImageCompareArea
                    mode="swipe"
                    baselineUri={selected?.testCase.baselineImageUrl}
                    actualUri={selected?.testCase.actualImageUrl}
                  />
                </TabsContent>
                <TabsContent value="overlay">
                  <ImageCompareArea
                    mode="overlay"
                    baselineUri={selected?.testCase.baselineImageUrl}
                    actualUri={selected?.testCase.actualImageUrl}
                  />
                </TabsContent>
                <TabsContent value="side-by-side">
                  <ImageCompareArea
                    mode="side-by-side"
                    baselineUri={selected?.testCase.baselineImageUrl}
                    actualUri={selected?.testCase.actualImageUrl}
                  />
                </TabsContent>
              </Tabs>

              <CommentsSection screenshotKey={screenshotKey} />
            </View>
          </ScrollView>
        )}

        <AlertDialogFooter>
          <AlertDialogAction
            onPress={onClose}
            testID="GoldenScreenshotViewer-Close"
          >
            Close
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export { GoldenScreenshotViewer };
