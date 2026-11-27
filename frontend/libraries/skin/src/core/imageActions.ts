import { useEffect, useRef, useState } from 'react';

import type { ChatImageViewerProps } from '../ports/ChatComponents';

/** What a viewer can do with its image, in the order it offers them, and what it says afterwards. */
export const IMAGE_ACTIONS = [
  { id: 'copy', label: 'Copy image', success: 'Image copied', failure: "Couldn't copy image" },
  { id: 'save', label: 'Save image', success: 'Image saved', failure: "Couldn't save image" },
  { id: 'share', label: 'Share…', success: '', failure: "Couldn't share image" },
] as const;

export type ImageAction = (typeof IMAGE_ACTIONS)[number];

/** Runs one image action at a time, closes the menu it was picked from and keeps what to say about it for a moment. */
export function useImageActions(actions: ChatImageViewerProps['actions'], setOpen: (open: boolean) => void) {
  const running = useRef(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  useEffect(() => {
    if (!message) return;
    const timer = setTimeout(() => setMessage(''), 2400);
    return () => clearTimeout(timer);
  }, [message]);
  return {
    busy,
    message,
    /** The actions this viewer was given, of the ones there are. */
    offered: IMAGE_ACTIONS.filter((row) => actions?.[row.id]),
    run(row: ImageAction) {
      const action = actions?.[row.id];
      if (running.current || !action) return;
      running.current = true;
      setBusy(true);
      setOpen(false);
      setMessage('');
      const done = (said: string) => {
        setMessage(said);
        running.current = false;
        setBusy(false);
      };
      void action().then(
        () => done(row.success),
        () => done(row.failure)
      );
    },
  };
}
