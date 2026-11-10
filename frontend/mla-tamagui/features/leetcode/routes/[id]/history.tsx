import { Logger } from '@lib/logging';
import { useLocalSearchParams } from 'expo-router';
import { useEffect, useRef } from 'react';

import { CodeHistorySheet } from '../../ui/CodeHistorySheet';
import { useCodeHistoryRequestStore } from '../../state/codeHistoryRequestStore';
import { dismissOverlayRoute } from '../../ui/dismissOverlayRoute';

export default function LeetCodeCodeHistoryRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const payload = useCodeHistoryRequestStore((s) => s.payload);
  const clear = useCodeHistoryRequestStore((s) => s.clear);

  // Distinguishes "opened without a restore target" from "we just cleared the
  // target on the way out". Without it, `clear()` re-fires the guard below and
  // the second router.back() pops PAST the editor to the problem list -- so
  // restoring a version dropped you on the wrong screen.
  const hadPayload = useRef(false);
  // Written in an effect, not during render: React 19's rules-of-hooks lint
  // rejects a ref write in the render pass, and the value is only read by the
  // guard effect below, which runs after this one on the same commit.
  useEffect(() => {
    if (payload) hadPayload.current = true;
  }, [payload]);

  // Deep-linked or cold-started with no editor to restore into: there is
  // nothing this sheet could do, so leave rather than render a Restore button
  // that silently does nothing.
  useEffect(() => {
    if (!hadPayload.current && (!payload || !id)) {
      Logger.warn('leetcode.history.no_target', { problemId: id });
      dismissOverlayRoute(id);
    }
  }, [payload, id]);

  if (!payload || !id) return null;

  return (
    <CodeHistorySheet
      problemId={id}
      onRestore={payload.onRestore}
      onClose={() => {
        dismissOverlayRoute(id);
        clear();
      }}
    />
  );
}