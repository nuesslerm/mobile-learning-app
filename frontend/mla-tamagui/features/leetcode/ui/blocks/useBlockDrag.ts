import { useMemo, useRef } from 'react';
import { Gesture, type LegacyPanGesture } from 'react-native-gesture-handler';

import type { Point } from '../../domain/blocks/blockLayout';
import type { BlockModel } from '../../domain/blocks/blockModel';
import { planDrop, type DragSource, type DropPlan } from '../../domain/blocks/dropPlan';

/** How far a finger moves before a drag starts: a tap, or a slip on the grip, is not one. */
export const DRAG_SLOP_PX = 8;

/** Where a drag is: the finger in list coordinates, and the slot it would drop at. */
export type DragState = { readonly source: DragSource; readonly pointer: Point; readonly plan: DropPlan | null };

/** What a drag tells the view: the model it plans against, where it is, and when it drops or ends. */
export type DragShell = {
  model(): BlockModel | null;
  begin(source: DragSource): void;
  move(drag: DragState): void;
  drop(plan: DropPlan): void;
  end(): void;
};

/**
 * The thin gesture shell of a grip or a chip: it turns the finger's displacement into a pointer in list coordinates (`origin` is where the grip's own box starts in them), asks the pure
 * planner where that would drop, and says so to the view. It runs on the JS thread because the planner and the view's state are not worklets.
 */
export function useBlockDrag(source: DragSource, origin: Point, shell: DragShell): LegacyPanGesture {
  // in a ref and not in the closure: in the web export a drag's updates ran in another closure than its begin
  const drag = useRef<{ grab: Point; down: Point; plan: DropPlan | null }>({ grab: origin, down: origin, plan: null });
  return useMemo(
    () =>
      Gesture.Pan()
        .runOnJS(true)
        .minDistance(DRAG_SLOP_PX)
        .maxPointers(1)
        .onBegin((event) => {
          drag.current = { grab: { x: origin.x + event.x, y: origin.y + event.y }, down: { x: event.absoluteX, y: event.absoluteY }, plan: null };
        })
        .onStart(() => shell.begin(source))
        .onUpdate((event) => {
          const model = shell.model();
          if (!model) return;
          const { grab, down } = drag.current;
          // the pan's own translation starts where it activated, a slop after the touch went down: the displacement is measured from the touch
          const pointer = { x: grab.x + event.absoluteX - down.x, y: grab.y + event.absoluteY - down.y };
          const plan = planDrop(model, source, pointer);
          drag.current.plan = plan;
          shell.move({ source, pointer, plan });
        })
        .onEnd((_event, success) => {
          const { plan } = drag.current;
          if (success && plan) shell.drop(plan);
        })
        .onFinalize(() => {
          drag.current.plan = null;
          shell.end();
        }),
    [source, origin, shell]
  );
}
