import { appConfig } from '@lib/app-config';
import type {
  InterpretResponse,
  PollResult,
  RunResponse,
  SubmitResponse,
  TestCase,
} from '../domain';
import { Logger } from '@lib/logging';
import { View } from '@lib/tamagui-design-system';
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react';

import {
  fetchFormat,
  fetchPoll,
  fetchRunLocal,
  fetchSubmit,
  fetchTest,
} from '../data/webBridgeClient';

export type LeetCodeBridgeHandle = {
  isReady: () => boolean;
  testCode: (
    slug: string,
    questionId: number,
    lang: string,
    code: string,
    dataInput: string
  ) => Promise<InterpretResponse>;
  submitCode: (
    slug: string,
    questionId: number,
    lang: string,
    code: string
  ) => Promise<SubmitResponse>;
  pollResult: (id: string) => Promise<PollResult>;
  waitForResult: (id: string, timeoutMs?: number) => Promise<PollResult>;
  /**
   * Run code against the problem's own test cases on the backend — no
   * leetcode.com, no session. Backs "Run Local".
   */
  runLocal: (
    lang: string,
    code: string,
    methodName: string,
    testCases: TestCase[]
  ) => Promise<RunResponse>;
  /** Format code on the backend. Backs "Format". */
  formatCode: (lang: string, code: string) => Promise<string>;
};

type Props = {
  slug: string;
  onReadyChange?: (ready: boolean) => void;
};

const DEFAULT_BACKEND_BASE_URL = 'http://localhost:8789';

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Thin fetch-only bridge to leetcode-backend (backend/leetcode-backend), the
 * session-owning service both platforms now call identically. No more
 * WebView, no cookie handling here at all — testCode/submitCode/pollResult
 * just relay to the backend; a 401 (no valid session) surfaces as a normal
 * rejected promise, exactly like the pre-migration proxy's 401s that callers
 * already handle. There's nothing left to probe locally (the backend owns
 * the session, not this component), so the bridge reports ready immediately
 * on mount.
 *
 * `slug` is accepted for interface parity with the pre-migration bridges
 * (the old native one mounted a WebView pointed at the problem page) but
 * isn't otherwise used here — the backend resolves the problem by slug
 * itself on each call.
 */
export const LeetCodeBridge = forwardRef<LeetCodeBridgeHandle, Props>(
  function LeetCodeBridge({ slug, onReadyChange }, ref) {
    const [, setReady] = useState(false);
    const readyRef = useRef(false);
    const backendBaseUrl =
      appConfig.leetcodeBackendBaseUrl ?? DEFAULT_BACKEND_BASE_URL;

    const updateReady = useCallback(
      (next: boolean) => {
        if (readyRef.current === next) return;
        Logger.info('bridge.ready.update', { next });
        readyRef.current = next;
        setReady(next);
        onReadyChange?.(next);
      },
      [onReadyChange]
    );

    useEffect(() => {
      updateReady(true);
      return () => updateReady(false);
    }, [updateReady]);

    const pollResult = useCallback(
      (id: string) => {
        Logger.info('bridge.poll.request', { id });
        return fetchPoll(backendBaseUrl, id);
      },
      [backendBaseUrl]
    );

    const waitForResult = useCallback(
      async (id: string, timeoutMs = 30000): Promise<PollResult> => {
        const deadline = Date.now() + timeoutMs;
        const startedAt = Date.now();
        let attempts = 0;
        Logger.info('bridge.poll.start', { id, timeoutMs });
        while (Date.now() < deadline) {
          await sleep(1500);
          attempts += 1;
          const result = await pollResult(id);
          if (result.state === 'SUCCESS' || result.state === 'FAILURE') {
            Logger.info('bridge.poll.done', {
              id,
              attempts,
              state: result.state,
              status: result.status_msg,
              ms: Date.now() - startedAt,
            });
            return result;
          }
        }
        Logger.warn('bridge.poll.timeout', {
          id,
          attempts,
          timeoutMs,
          ms: Date.now() - startedAt,
        });
        throw new Error('Timed out waiting for judge result');
      },
      [pollResult]
    );

    useImperativeHandle(
      ref,
      () => ({
        isReady: () => {
          const ready = readyRef.current;
          Logger.info('bridge.isReady', { ready });
          return ready;
        },
        testCode: (slug2, questionId, lang, code, dataInput) => {
          Logger.info('bridge.test.request', { slug: slug2, questionId, lang });
          return fetchTest(backendBaseUrl, {
            slug: slug2,
            lang,
            questionId,
            code,
            dataInput,
          });
        },
        submitCode: (slug2, questionId, lang, code) => {
          Logger.info('bridge.submit.request', {
            slug: slug2,
            questionId,
            lang,
          });
          return fetchSubmit(backendBaseUrl, {
            slug: slug2,
            lang,
            questionId,
            code,
          });
        },
        pollResult,
        waitForResult,
        runLocal: (lang, code, methodName, testCases) => {
          Logger.info('bridge.runLocal.request', {
            lang,
            methodName,
            testCaseCount: testCases.length,
          });
          return fetchRunLocal(backendBaseUrl, {
            lang,
            code,
            methodName,
            testCases,
          });
        },
        formatCode: (lang, code) => {
          Logger.info('bridge.format.request', { lang });
          return fetchFormat(backendBaseUrl, { lang, code });
        },
      }),
      [backendBaseUrl, pollResult, waitForResult]
    );

    // No WebView or visible element to mount any more — this is a
    // logic-only bridge component on every platform now (generalized from
    // the pre-migration web bridge's own logic-only return).
    void slug;

    return <View testID="LeetCodeBridge" pointerEvents="none" />;
  }
);
