import { useEffect, useState } from 'react';
import { Platform } from 'react-native';

import type { WorkspaceActionsProps } from '../ports/HostWorkspaceList';

/** Renaming a workspace from its actions, whatever the sheet looks like: the page, the draft, and a save that waits for the one before it. */
export function useWorkspaceRename({ name, saving, onRename }: Pick<WorkspaceActionsProps, 'name' | 'saving' | 'onRename'>) {
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(name);
  return {
    renaming,
    start: () => setRenaming(true),
    draft,
    setDraft,
    save: () => {
      if (!saving) onRename(draft.trim());
    },
  };
}

/**
 * A touch held on a row opens a sheet under the finger, and on the web its release would press whatever control just mounted there. The
 * returned setter takes the sheet's element: inside it a click counts only on the control the press began on. Nothing to set off the web.
 */
export function useHeldTouchGuard(): ((element: unknown) => void) | undefined {
  const [panel, setPanel] = useState<unknown>(null);
  useEffect(() => {
    if (Platform.OS !== 'web' || !(panel instanceof HTMLElement)) return;
    let pressed: Element | null = null;
    const control = (event: Event) => (event.target instanceof Element ? event.target.closest('[role="button"], [role="radio"], button, input') : null);
    const down = (event: PointerEvent) => {
      pressed = control(event);
    };
    const cancel = () => {
      pressed = null;
    };
    const click = (event: MouseEvent) => {
      if (event.detail > 0 && (!pressed || pressed !== control(event))) {
        event.preventDefault();
        event.stopPropagation();
      }
      pressed = null;
    };
    panel.addEventListener('pointerdown', down, true);
    panel.addEventListener('pointercancel', cancel, true);
    panel.addEventListener('click', click, true);
    return () => {
      panel.removeEventListener('pointerdown', down, true);
      panel.removeEventListener('pointercancel', cancel, true);
      panel.removeEventListener('click', click, true);
    };
  }, [panel]);
  return Platform.OS === 'web' ? setPanel : undefined;
}
