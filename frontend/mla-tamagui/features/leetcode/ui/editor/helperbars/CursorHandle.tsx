import { Icon, View } from '@lib/tamagui-design-system';
import { Move } from '@lib/tamagui-design-system/icons';
import React, { useMemo, useRef } from 'react';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';

import type { CaretDragSession, EditorInputHandle } from '../EditorInput';

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
};

/**
 * The cursor pad at the left end of the action row: a drag on it moves the caret, sideways by characters and then words, up and down
 * by lines. The finger's displacement from the grab goes to the editor, which maps it (domain/editor/cursorDrag).
 */
export function CursorHandle({ editorRef }: Props) {
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

  return (
    <GestureDetector gesture={pan}>
      <View
        testID="EditorCursorHandle"
        width={56}
        items="center"
        justify="center"
        bg="$color3"
        borderRightWidth={1}
        borderColor="$borderColor"
      >
        <Icon as={Move} size={20} color="$color" />
      </View>
    </GestureDetector>
  );
}
