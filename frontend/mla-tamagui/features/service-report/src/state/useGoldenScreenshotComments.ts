/**
 * Loads/posts comments for one golden-screenshot capture. Plain component
 * state, same rationale as `useHealthReportHistory` — scoped to whichever
 * capture the viewer currently has open, not shared app state.
 */

import { Logger } from '@lib/logging';
import type { GoldenScreenshotComment } from '../contracts';
import { useCallback, useEffect, useState } from 'react';

import {
  fetchGoldenScreenshotComments,
  postGoldenScreenshotComment,
} from '../data/goldenScreenshotCommentsClient';


type GoldenScreenshotCommentsState =
  | { status: 'loading' }
  | { status: 'not-configured'; reason: string }
  | { status: 'unreachable'; error: string }
  | { status: 'ready'; comments: GoldenScreenshotComment[] };

type PostCommentResult = { ok: true } | { ok: false; error: string };

export type UseGoldenScreenshotCommentsResult =
  GoldenScreenshotCommentsState & {
    refresh: () => void;
    post: (input: {
      author?: string;
      commentText: string;
    }) => Promise<PostCommentResult>;
  };

/** `screenshotKey` undefined means no capture is selected yet (e.g. no history entries) — stays `loading` without fetching. */
export function useGoldenScreenshotComments(
  screenshotKey: string | undefined
): UseGoldenScreenshotCommentsResult {
  const [state, setState] = useState<GoldenScreenshotCommentsState>({
    status: 'loading',
  });

  const refresh = useCallback(() => {
    if (!screenshotKey) return;
    setState({ status: 'loading' });
    fetchGoldenScreenshotComments(screenshotKey)
      .then((result) => {
        if (result.status === 'ready') {
          setState({ status: 'ready', comments: result.comments });
        } else {
          setState(result);
        }
      })
      .catch((err: unknown) => {
        Logger.error('service_report.comments_load_failed', { err });
        setState({
          status: 'unreachable',
          error: err instanceof Error ? err.message : String(err),
        });
      });
  }, [screenshotKey]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const post = useCallback(
    async (input: {
      author?: string;
      commentText: string;
    }): Promise<PostCommentResult> => {
      if (!screenshotKey) {
        return { ok: false, error: 'No screenshot selected' };
      }
      const result = await postGoldenScreenshotComment({
        screenshotKey,
        ...input,
      });
      if (result.status === 'ok') {
        refresh();
        return { ok: true };
      }
      const error =
        result.status === 'not-configured' ? result.reason : result.error;
      Logger.warn('service_report.comment_post_result', {
        status: result.status,
        error,
      });
      return { ok: false, error };
    },
    [screenshotKey, refresh]
  );

  return { ...state, refresh, post };
}
