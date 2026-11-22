import { Icon, View } from '@lib/tamagui-design-system';
import { Move } from '@lib/tamagui-design-system/icons';
import React, { useCallback, useMemo, useRef } from 'react';
import { View as NativeView, type AccessibilityActionEvent } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';

import { CURSOR_ACTIONS, motionOfAction } from '../../../domain/editor/cursorActions';
import type { CaretDragSession, EditorInputHandle } from '../EditorInput';

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
  /** What a screen reader says the handle's value is: where the caret is, by line and column. */
  valueText: string;
};

const PAD = { width: 56 } as const;

const HINT = 'Drag sideways to move the caret by characters and words, up or down to move it by lines';

/**
 * The cursor pad at the left end of the action row: a drag on it moves the caret, sideways by characters and then words, up and down
 * by lines. The finger's displacement from the grab goes to the editor, which maps it (domain/editor/cursorDrag). To a screen reader it
 * is an adjustable control whose actions make the same moves. The accessibility node is a plain React Native view, which takes the
 * role and the actions as the platform names them.
 */
export function CursorHandle({ editorRef, valueText }: Props) {
  const dragRef = useRef<CaretDragSession | null>(null);

  // runOnJS(true): the session lives in JS and its calls cannot be worklets
  const pan = useMemo(
    () =>
      Gesture.Pan()
        .runOnJS(true)
        .minDistance(0)
        .maxPointers(1)
        .onBegin(() => {
          dragRef.current = editorRef.current?.beginCaretDrag() ?? null;
        })
        .onUpdate((event) => {
          dragRef.current?.move({ dx: event.translationX, dy: event.translationY });
        })
        .onFinalize(() => {
          const drag = dragRef.current;
          dragRef.current = null;
          void drag?.end();
        }),
    [editorRef]
  );

  const onAccessibilityAction = useCallback(
    (event: AccessibilityActionEvent) => {
      const motion = motionOfAction(event.nativeEvent.actionName);
      if (motion) void editorRef.current?.moveCaret(motion);
    },
    [editorRef]
  );

  return (
    <GestureDetector gesture={pan}>
      <NativeView
        testID="EditorCursorHandle"
        style={PAD}
        accessible
        accessibilityRole="adjustable"
        aria-label="Cursor handle"
        accessibilityHint={HINT}
        aria-valuetext={valueText}
        accessibilityActions={CURSOR_ACTIONS}
        onAccessibilityAction={onAccessibilityAction}
      >
        <View flex={1} items="center" justify="center" bg="$color3" borderRightWidth={1} borderColor="$borderColor">
          <Icon as={Move} size={20} color="$color" />
        </View>
      </NativeView>
    </GestureDetector>
  );
}
