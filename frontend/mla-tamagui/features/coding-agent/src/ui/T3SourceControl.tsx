import { BackHandler, Platform } from 'react-native';
import { useAgentComponents, type SourceCommit } from '@lib/agent-ui-contracts';
import { Logger } from '@lib/logging';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  CheckoutHeadChanged,
  type CheckoutChanges,
  type CheckoutCommit,
  type CheckoutHistory,
  type HistoryCursor,
} from '../domain/checkoutHistory';
import type { Pairing } from '../domain/mobile';
import type { T3MobilePort } from '../domain/t3Mobile';
import type { CheckoutRow } from '../domain/workspaceCatalog';

export function T3SourceControl({
  api,
  pair,
  checkout,
  onBack,
}: {
  api: T3MobilePort;
  pair: Pairing;
  checkout: CheckoutRow;
  onBack: () => void;
}) {
  const { SourceControl } = useAgentComponents();
  const [page, setPage] = useState<'changes' | 'commits' | 'detail'>('changes');
  const [selected, setSelected] = useState<SourceCommit>();
  const [changes, setChanges] = useState<CheckoutChanges>();
  const [detail, setDetail] = useState<CheckoutCommit>();
  const [busy, setBusy] = useState(true);
  const [notice, setNotice] = useState<string>();
  const [refreshNumber, refresh] = useState(0);
  const back = useCallback(() => {
    if (page === 'detail') setPage('commits');
    else if (page === 'commits') setPage('changes');
    else onBack();
  }, [page, onBack]);
  useEffect(() => {
    if (Platform.OS !== 'android') return;
    const subscription = BackHandler.addEventListener(
      'hardwareBackPress',
      () => {
        back();
        return true;
      }
    );
    return () => subscription.remove();
  }, [back]);

  useEffect(() => {
    if (page === 'commits') return;
    const controller = new AbortController();
    setBusy(true);
    setNotice(undefined);
    const read = async () => {
      if (page === 'changes') {
        const result = await api.checkoutChanges(
          pair,
          checkout.id,
          controller.signal
        );
        if (!controller.signal.aborted) setChanges(result);
      } else if (selected) {
        const result = await api.checkoutCommit(
          pair,
          checkout.id,
          selected.sha,
          controller.signal
        );
        if (!controller.signal.aborted) setDetail(result);
      }
    };
    void read()
      .catch(() => {
        if (controller.signal.aborted) return;
        Logger.warn('coding_agent.checkout_comparison_failed');
        setNotice('Could not load changes.');
      })
      .finally(() => {
        if (!controller.signal.aborted) setBusy(false);
      });
    return () => controller.abort();
  }, [api, pair, checkout.id, selected, page, refreshNumber]);

  const request = useRef<AbortController | undefined>(undefined);
  const [history, setHistory] = useState<CheckoutHistory>();
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string>();
  const load = useCallback(
    async (cursor: HistoryCursor | null) => {
      if (cursor && request.current) return;
      request.current?.abort();
      const controller = new AbortController();
      request.current = controller;
      setError(undefined);
      setLoading(!cursor);
      setLoadingMore(!!cursor);
      try {
        let page: CheckoutHistory;
        try {
          page = await api.checkoutHistory(
            pair,
            checkout.id,
            cursor,
            controller.signal
          );
        } catch (cause) {
          if (
            !(cause instanceof CheckoutHeadChanged) ||
            !cursor ||
            controller.signal.aborted
          )
            throw cause;
          cursor = null;
          page = await api.checkoutHistory(
            pair,
            checkout.id,
            null,
            controller.signal
          );
        }
        if (!controller.signal.aborted) {
          setHistory((prev) =>
            cursor && prev?.head === page.head
              ? { ...page, commits: [...prev.commits, ...page.commits] }
              : page
          );
        }
      } catch {
        if (!controller.signal.aborted) {
          Logger.warn('coding_agent.checkout_history_failed');
          setError('Could not load commits.');
        }
      } finally {
        if (request.current === controller) {
          request.current = undefined;
          setLoading(false);
          setLoadingMore(false);
        }
      }
    },
    [api, pair, checkout.id]
  );

  useEffect(() => {
    return () => {
      request.current?.abort();
      request.current = undefined;
    };
  }, []);
  const cursor =
    history?.head && history.nextOffset !== null
      ? { head: history.head, offset: history.nextOffset }
      : null;
  return (
    <SourceControl
      page={page}
      selected={selected}
      detail={detail}
      comparison={changes?.comparison}
      openHistory={() => {
        setPage('commits');
        if (!history && !request.current) void load(null);
      }}
      openCommit={(commit) => {
        setSelected(commit);
        setDetail(undefined);
        setPage('detail');
      }}
      head={
        page === 'commits'
          ? (history?.head ?? changes?.head ?? null)
          : (changes?.head ?? null)
      }
      branch={
        page === 'commits' && history
          ? history.branch
          : changes
            ? changes.branch
            : checkout.branch
      }
      commits={history?.commits ?? []}
      loading={page === 'commits' ? loading : busy}
      loadingMore={loadingMore}
      error={page === 'commits' ? error : notice}
      onBack={back}
      onRefresh={() =>
        page === 'commits' ? void load(null) : refresh((n) => n + 1)
      }
      onLoadMore={cursor ? () => void load(cursor) : undefined}
    />
  );
}
