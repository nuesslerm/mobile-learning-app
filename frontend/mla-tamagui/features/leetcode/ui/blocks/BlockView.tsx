import type { Language } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';
import { Text, View } from '@lib/tamagui-design-system';
import { YStack } from '@lib/tamagui-design-system/primitives';
import React from 'react';

import { applyDrop, describeOutcome } from '../../domain/blocks/applyDrop';
import type { BlockEditor } from '../../domain/blocks/blockEditor';
import { GUTTER, listHeight, ROW_HEIGHT } from '../../domain/blocks/blockLayout';
import type { BlockModel } from '../../domain/blocks/blockModel';
import type { DropPlan } from '../../domain/blocks/dropPlan';
import { paletteItem } from '../../domain/blocks/palette';
import { useBlockModel, type BlockModelStatus } from '../../state/blocks/useBlockModel';
import type { EditorInputHandle } from '../editor/EditorInput';
import { BlockDropIndicator } from './BlockDropIndicator';
import { BlockPalette } from './BlockPalette';
import { BlockRow } from './BlockRow';
import type { DragShell, DragState } from './useBlockDrag';

type Props = {
  /** the person turned the block view on */
  enabled: boolean;
  language: Language;
  /** the editor page has hydrated: its typed API answers */
  ready: boolean;
  editorRef: React.RefObject<EditorInputHandle | null>;
};

function messageFor(status: BlockModelStatus, rows: number): string | null {
  if (status === 'unavailable') return 'This editor cannot show blocks.';
  if (status === 'failed') return 'The code could not be read. Type again to try once more.';
  if (status === 'ready' && rows === 0) return 'No statements yet. Type one in the editor.';
  return null;
}

const sameDrag = (a: DragState | null, b: DragState): boolean => a !== null && a.plan?.slot.id === b.plan?.slot.id && a.pointer.x === b.pointer.x && a.pointer.y === b.pointer.y;

/**
 * The code as rows under the editor, for Python while the setting is on and the editor is ready. It reads the editor through its typed API and a drop is one call to it. It
 * sits between the editor zone and the tap-away Pressable, in neither: a tap on it is its own and does not lower the keyboard.
 */
export function BlockView({ enabled, language, ready, editorRef }: Props) {
  const visible = enabled && language === 'python' && ready;
  const [editor, setEditor] = React.useState<BlockEditor | null>(null);
  React.useEffect(() => {
    setEditor(visible ? (editorRef.current?.api() ?? null) : null);
  }, [visible, editorRef]);
  const { status, model, hold, release, refresh } = useBlockModel(editor);

  // the drag reads these when it moves or drops, so a gesture built once always plans against the model on show
  const modelRef = React.useRef<BlockModel | null>(null);
  const editorHeldRef = React.useRef<BlockEditor | null>(null);
  React.useLayoutEffect(() => {
    modelRef.current = model;
    editorHeldRef.current = editor;
  }, [model, editor]);

  const droppedRef = React.useRef(false);
  const [drag, setDrag] = React.useState<DragState | null>(null);
  const [dropMessage, setDropMessage] = React.useState<string | null>(null);

  const drop = React.useCallback(
    async (plan: DropPlan) => {
      const planned = modelRef.current;
      const target = editorHeldRef.current;
      if (!planned || !target) return;
      const started = Date.now();
      const outcome = await applyDrop(target.client, planned, plan);
      Logger.info('leetcode.blocks.drop', {
        op: plan.source.kind === 'block' ? 'move' : 'insert',
        outcome: outcome.kind,
        ms: Date.now() - started,
        ...(outcome.kind === 'refused' ? { reason: outcome.reason } : {}),
        ...(outcome.kind === 'outdated' || outcome.kind === 'unavailable' ? { code: outcome.code } : {}),
      });
      setDropMessage(describeOutcome(outcome));
      // a move tells the model through the editor's changed event; an outdated drop changed nothing the model would hear of
      if (outcome.kind === 'outdated') refresh();
    },
    [refresh]
  );

  const shell = React.useMemo<DragShell>(
    () => ({
      model: () => modelRef.current,
      begin: (picked) => {
        Logger.info('leetcode.blocks.drag_begin', { source: picked.kind });
        droppedRef.current = false;
        setDropMessage(null);
        hold();
      },
      move: (next) => setDrag((previous) => (sameDrag(previous, next) ? previous : next)),
      drop: (plan) => {
        droppedRef.current = true;
        void drop(plan);
      },
      end: () => {
        Logger.info('leetcode.blocks.drag_end', { dropped: droppedRef.current });
        setDrag(null);
        release();
      },
    }),
    [hold, release, drop]
  );

  if (!visible) return null;

  const rows = model?.rows ?? [];
  const message = dropMessage ?? messageFor(status, rows.length);
  const source = drag?.source;
  const draggedLabel = source?.kind === 'block' ? rows.find((row) => row.id === source.id)?.label : source?.kind === 'palette' ? paletteItem(source.item)?.label : undefined;
  return (
    <YStack mx="$3" mb="$2" rounded="$3" borderWidth={1} borderColor="$borderColor" bg="$background" overflow="hidden" testID="ProblemEditorBlockView">
      {model ? <BlockPalette shell={shell} /> : null}
      <View height={listHeight(rows.length)} testID="BlockViewRows">
        {rows.map((row) => (
          <BlockRow key={row.id} row={row} shell={shell} />
        ))}
        {drag && draggedLabel !== undefined ? (
          <View position="absolute" l={GUTTER} r="$2" t={drag.pointer.y - ROW_HEIGHT / 2} height={ROW_HEIGHT} px="$2" justify="center" rounded="$2" borderWidth={1} borderColor="$blue9" bg="$color3" opacity={0.9} pointerEvents="none" testID="BlockGhost">
            <Text variant="code" numberOfLines={1}>
              {draggedLabel}
            </Text>
          </View>
        ) : null}
        {drag?.plan ? <BlockDropIndicator plan={drag.plan} /> : null}
      </View>
      {message ? (
        <Text variant="muted" px="$3" py="$2" testID="BlockViewMessage">
          {message}
        </Text>
      ) : null}
    </YStack>
  );
}
