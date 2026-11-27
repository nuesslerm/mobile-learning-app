import { RcaUnavailable, WorktreeMissing } from '../domain/t3Mobile';
import { BackHandler, Linking, Platform } from 'react-native';
import { useSkin, type SourceCommit } from '@lib/skin';
import { Logger } from '@lib/logging';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  CheckoutHeadChanged,
  parsePullRequestReference,
  type CheckoutPullRequest,
  type CheckoutFile,
  type CheckoutBranches,
  type CheckoutChanges,
  type CheckoutCommit,
  type CheckoutHistory,
  type HistoryCursor,
} from '../domain/checkoutHistory';
import { buildMobileDiffLines } from '../domain/diff/mobile-diff-lines';
import type { Pairing } from '../domain/mobile';
import type { T3MobilePort } from '../domain/t3Mobile';
import type { CheckoutRow } from '../domain/workspaceCatalog';

export function T3SourceControl({
  api,
  pair,
  checkout,
  backLabel,
  onBack,
}: {
  api: T3MobilePort;
  pair: Pairing;
  checkout: CheckoutRow;
  backLabel?: string;
  onBack: (branchMayHaveChanged: boolean) => void;
}) {
  const { SourceControl } = useSkin();
  const [page, setPage] = useState<'changes' | 'commits' | 'detail'>('changes');
  const [tab, setTab] = useState<'changes' | 'pr'>('changes');
  const [pullRequest, setPullRequest] = useState<CheckoutPullRequest>();
  const [prError, setPrError] = useState<string>();
  const [prRevision, reloadPr] = useState(0);
  useEffect(() => {
    if (tab !== 'pr') return;
    const controller = new AbortController();
    setPullRequest(undefined);
    setPrError(undefined);
    api.checkoutPullRequest(pair, checkout.id, controller.signal).then(result => {
      if (!controller.signal.aborted) setPullRequest(result);
    }).catch(cause => {
      if (controller.signal.aborted) return;
      Logger.withError(cause).warn('coding_agent.checkout_pull_request_failed');
      setPrError('Could not load pull request.');
    });
    return () => controller.abort();
  }, [api, pair, checkout.id, tab, prRevision]);
  const [selected, setSelected] = useState<SourceCommit>();
  const [changes, setChanges] = useState<CheckoutChanges>();
  const [detail, setDetail] = useState<CheckoutCommit>();
  const [busy, setBusy] = useState(true);
  const [notice, setNotice] = useState<string>();
  const [refreshNumber, refresh] = useState(0);
  const [fileIndex, selectFile] = useState<number>();
  const [viewed, setViewed] = useState<Set<string>>(new Set());
  const [fileData, setFileData] = useState<CheckoutFile>();
  const [fileError, setFileError] = useState(false);
  const [retryNumber, retryFile] = useState(0);
  const files = page === 'detail' ? detail?.files : changes?.comparison?.files;
  const file = fileIndex === undefined ? undefined : files?.[fileIndex];
  const head = page === 'detail' ? selected?.sha : changes?.head;
  const base =
    page === 'detail' ? selected?.parent : changes?.comparison?.baseOid;
  const scope = page === 'detail' ? selected?.sha : changes?.head;
  const viewedKey = (path: string) => JSON.stringify([scope, base, path]);
  function openFile(index: number) {
    if (index === fileIndex) return;
    setFileData(undefined);
    setFileError(false);
    selectFile(index);
  }
  useEffect(() => {
    if (!file) return;
    if (!head || (page === 'changes' && !base)) {
      setFileError(true);
      return;
    }
    const controller = new AbortController();
    void api
      .checkoutFile(
        pair,
        checkout.id,
        {
          head,
          base: base ?? undefined,
          path: file.path,
          oldPath: file.orig_path,
        },
        controller.signal
      )
      .then((data) => {
        if (!controller.signal.aborted) setFileData(data);
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          Logger.warn('coding_agent.checkout_file_failed');
          setFileError(true);
        }
      });
    return () => controller.abort();
  }, [api, pair, checkout.id, file, head, base, page, retryNumber]);
  const lines = useMemo(
    () =>
      fileData?.kind === 'text'
        ? buildMobileDiffLines(fileData.original ?? '', fileData.modified ?? '')
            .lines
        : undefined,
    [fileData]
  );
  function mark(path: string, value: boolean) {
    setViewed((previous) => {
      const next = new Set(previous);
      if (value) next.add(viewedKey(path));
      else next.delete(viewedKey(path));
      return next;
    });
  }
  const refreshCatalogOnExit = useRef(false);
  const switchPending = useRef(false);
  const exitAfterSwitch = useRef(false);
  const back = useCallback(() => {
    if (fileIndex !== undefined) selectFile(undefined);
    else if (page === 'detail') setPage('commits');
    else if (page === 'commits') setPage('changes');
    else if (switchPending.current) exitAfterSwitch.current = true;
    else onBack(refreshCatalogOnExit.current);
  }, [fileIndex, page, onBack]);
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
      .catch((cause) => {
        if (controller.signal.aborted) return;
        Logger.warn('coding_agent.checkout_comparison_failed');
        setNotice(cause instanceof WorktreeMissing || cause instanceof RcaUnavailable ? cause.message : 'Could not load changes.');
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
  const [pickerOpen, setPickerOpen] = useState(false);
  const [branches, setBranches] = useState<CheckoutBranches>();
  const [branchError, setBranchError] = useState<string>();
  const [switchError, setSwitchError] = useState<string>();
  const [switching, setSwitching] = useState(false);
  const branchRequest = useRef<AbortController | undefined>(undefined);
  useEffect(() => () => branchRequest.current?.abort(), [checkout.id]);
  function closeBranches() {
    branchRequest.current?.abort();
    setPickerOpen(false);
  }
  async function openBranches() {
    branchRequest.current?.abort();
    const controller = new AbortController();
    branchRequest.current = controller;
    setSwitchError(undefined);
    setBranchError(undefined);
    setBranches(undefined);
    setPickerOpen(true);
    try {
      const result = await api.checkoutBranches(pair, checkout.id, controller.signal);
      if (!controller.signal.aborted) setBranches(result);
    } catch {
      if (!controller.signal.aborted) {
        Logger.warn('coding_agent.checkout_branches_failed');
        setBranchError('Could not load branches.');
      }
    }
  }
  async function selectBranch(branch: string) {
    if (switchPending.current) return;
    closeBranches();
    if (branch === branches?.current) return;
    const controller = new AbortController();
    branchRequest.current = controller;
    switchPending.current = true;
    setSwitching(true);
    try {
      const moved = await api.switchCheckoutBranch(pair, checkout.id, branch, controller.signal);
      if (!controller.signal.aborted) setChanges({ ...moved, comparison: null });
    } catch (cause) {
      if (!controller.signal.aborted) {
        Logger.warn('coding_agent.switch_checkout_branch_failed');
        setSwitchError(cause instanceof Error ? cause.message : 'Could not switch branch.');
      }
    } finally {
      if (!controller.signal.aborted) {
        switchPending.current = false;
        refreshCatalogOnExit.current = true;
        if (exitAfterSwitch.current) {
          onBack(true);
        } else {
          setSwitching(false);
          request.current?.abort();
          request.current = undefined;
          setHistory(undefined);
          refresh((n) => n + 1);
        }
      }
    }
  }
  const cursor =
    history?.head && history.nextOffset !== null
      ? { head: history.head, offset: history.nextOffset }
      : null;
  return (
    <SourceControl
      backLabel={backLabel}
      tab={tab}
      selectTab={next => {
        if (next !== tab) {
          setPullRequest(undefined);
          setPrError(undefined);
          setTab(next);
        }
      }}
      prPanel={{
        result: pullRequest,
        parseReference: parsePullRequestReference,
        link: async number => {
          try {
            await api.linkCheckoutPullRequest(pair, checkout.id, number);
          } catch (cause) {
            Logger.withError(cause).warn('coding_agent.checkout_link_pull_request_failed');
            throw cause;
          }
          reloadPr(n => n + 1);
        },
        error: prError,
        refresh: () => reloadPr(n => n + 1),
        open: url => {
          Linking.openURL(url).catch(cause => {
            Logger.withError(cause).warn('coding_agent.checkout_pull_request_open_failed');
            setPrError('Could not open pull request.');
          });
        },
      }}
      openFile={openFile}
      openBranches={() => void openBranches()}
      switchingBranch={switching}
      picker={pickerOpen ? {
        branches: branches?.branches,
        current: branches?.current,
        truncated: branches?.truncated,
        error: branchError,
        retry: () => void openBranches(),
        close: closeBranches,
        select: (name) => void selectBranch(name),
      } : undefined}
      review={
        file && files && fileIndex !== undefined
          ? {
              files,
              index: fileIndex,
              branch: page === 'changes',
              viewed: new Set(
                files
                  .filter((f) => viewed.has(viewedKey(f.path)))
                  .map((f) => f.path)
              ),
              lines,
              loading: !fileData && !fileError,
              message: fileError
                ? 'Could not load file diff.'
                : fileData?.kind === 'binary'
                  ? 'Binary file'
                  : fileData?.kind === 'tooLarge'
                    ? 'File is too large to preview.'
                    : undefined,
              retry: fileError
                ? () => {
                    setFileError(false);
                    retryFile((n) => n + 1);
                  }
                : undefined,
              back,
              select: openFile,
              toggleViewed: () =>
                mark(file.path, !viewed.has(viewedKey(file.path))),
              advance: () => {
                if (page === 'changes') mark(file.path, true);
                openFile((fileIndex + 1) % files.length);
              },
            }
          : undefined
      }
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
      error={page === 'commits' ? error : switchError ?? notice}
      onBack={back}
      onRefresh={() => {
        setSwitchError(undefined);
        if (page === 'commits') void load(null);
        else refresh((n) => n + 1);
      }}
      onLoadMore={cursor ? () => void load(cursor) : undefined}
    />
  );
}
