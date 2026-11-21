import { hydrationErrors, screen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import * as lessonRoute from '../../app/(app)/(content)/tamil/[lessonId]';
import * as trackRoute from '../../app/(app)/(content)/tamil/tracks/[trackId]';
import * as writeRoute from '../../app/(app)/(content)/tamil/write/[subjectId]';
import { TamilCourse } from '../../features/tamil-writing/domain/course';
import { TRACKS } from '../../features/tamil-writing/domain/tracks';

import { app } from './app';

type Listed = { generateStaticParams?: () => Record<string, string | string[]>[] };

function listedIds(route: Listed, key: string): string[] {
  return (route.generateStaticParams?.() ?? []).map((params) => String(params[key])).sort();
}

describe('Tamil routes opened cold from the web export', () => {
  it('given the lesson route, when the export lists its pages, then every lesson has its own', () => {
    expect(listedIds(lessonRoute, 'lessonId')).toEqual(TamilCourse.getLessons().map((lesson) => lesson.id).sort());
  });

  it('given the track route, when the export lists its pages, then every track has its own', () => {
    expect(listedIds(trackRoute, 'trackId')).toEqual(TRACKS.map((track) => track.id).sort());
  });

  it('given the write route, when the export lists its pages, then every letter and word has its own', () => {
    const subjects = [...new Set(TRACKS.flatMap((track) => track.subjects.map((subject) => subject.id)))].sort();
    expect(listedIds(writeRoute, 'subjectId')).toEqual(subjects);
  });

  it.each([
    ['uyir-1', 'அ'],
    ['uyirmei-k', 'க'],
  ])('given a cold load of /tamil/%s, when the browser hydrates the prerendered page, then React reports nothing and the lesson shows', async (lessonId, glyph) => {
    const caught = hydrationErrors();
    await app.render(`/tamil/${lessonId}`, { prerender: 'export' });
    await waitFor(() => expect(screen.getByTestId('TamilPracticeGlyph').textContent).toBe(glyph));
    caught.stop();
    expect(caught.seen).toEqual([]);
  });

  it.each(['uyir-a', 'word-uyir-a'])(
    'given a cold load of /tamil/write/%s, when the browser hydrates the prerendered page, then React reports nothing and the pad shows',
    async (subjectId) => {
      const caught = hydrationErrors();
      await app.render(`/tamil/write/${subjectId}`, { prerender: 'export' });
      await waitFor(() => expect(screen.getByTestId('TamilWriteScreen')).toBeTruthy());
      caught.stop();
      expect(caught.seen).toEqual([]);
    }
  );

  it('given a cold load of a track, when the browser hydrates the prerendered page, then React reports nothing and the tiles show', async () => {
    const caught = hydrationErrors();
    await app.render('/tamil/tracks/vowels', { prerender: 'export' });
    await waitFor(() => expect(screen.getByTestId('TamilTrackScreen')).toBeTruthy());
    caught.stop();
    expect(caught.seen).toEqual([]);
  });

  it.each([
    ['/tamil/no-such-lesson', 'TamilLessonMissing'],
    ['/tamil/write/no-such-subject', 'TamilWriteMissing'],
    ['/tamil/tracks/no-such-track', 'TamilTrackMissing'],
  ])('given a cold load of %s, an id the export has no page for, when the browser hydrates the template page, then React reports nothing and the screen says it is missing', async (path, screenId) => {
    const caught = hydrationErrors();
    await app.render(path, { prerender: 'export' });
    await waitFor(() => expect(screen.getByTestId(screenId)).toBeTruthy());
    caught.stop();
    expect(caught.seen).toEqual([]);
  });
});
