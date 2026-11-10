import { hydrationErrors, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { app } from './app';

// Every dynamic route and a path to open it cold. A new `[param]` route fails the first test until it is listed here.
const COLD_PATHS: Record<string, string> = {
  '/coding-agent/[threadId]': '/coding-agent/thread-1',
  '/coding-agent/[threadId]/settings': '/coding-agent/thread-1/settings',
  '/coding-agent/runs/[runId]': '/coding-agent/runs/run-1',
  '/leetcode/[id]': '/leetcode/two-sum',
  '/leetcode/[id]/history': '/leetcode/two-sum/history',
  '/leetcode/[id]/rename': '/leetcode/two-sum/rename',
  '/leetcode/[id]/reset-code': '/leetcode/two-sum/reset-code',
  '/leetcode/[id]/unsaved': '/leetcode/two-sum/unsaved',
  '/tamil/[lessonId]': '/tamil/uyir-1',
  '/tamil/tracks/[trackId]': '/tamil/tracks/vowels',
  '/tamil/write/[subjectId]': '/tamil/write/uyir-a',
};

// Mounts a mermaid WebView whose CDN script jsdom cannot load; the real export opens it cold in `wf test run`
const NOT_IN_JSDOM = ['/leetcode/[id]/description'];

const routePattern = (file: string): string =>
  '/' +
  file
    .replace('../../app/', '')
    .replace(/\.tsx$/, '')
    .split('/')
    .filter((segment) => !/^\(.*\)$/.test(segment))
    .join('/');

const dynamicRoutes = Object.keys(import.meta.glob('../../app/**/*.tsx'))
  .map(routePattern)
  .filter((pattern) => pattern.includes('['))
  .sort();

describe('Dynamic routes opened cold from the web export', () => {
  it('given the app routes, when one has a [param], then it has a cold path listed here', () => {
    expect(dynamicRoutes).toEqual([...Object.keys(COLD_PATHS), ...NOT_IN_JSDOM].sort());
  });

  it.each(Object.entries(COLD_PATHS))(
    'given a cold load of %s at %s, when the browser hydrates the page the export holds for it, then React reports nothing',
    async (_pattern, path) => {
      const caught = hydrationErrors();
      await app.render(path, { prerender: 'export' });
      await waitFor(() => expect(document.body.textContent).not.toBe(''));
      caught.stop();
      expect(caught.seen).toEqual([]);
    }
  );

  // docs/gaps/web-export-cold-deep-link-hydration-418.md: the export prerenders /tamil/review without its query
  it.fails('given a cold load of /tamil/review?lessonId=, when the browser hydrates the prerendered page, then React reports nothing', async () => {
    const caught = hydrationErrors();
    await app.render('/tamil/review', { prerender: 'export', search: { lessonId: 'uyir-1' } });
    await waitFor(() => expect(document.body.textContent).not.toBe(''));
    caught.stop();
    expect(caught.seen).toEqual([]);
  });
});
