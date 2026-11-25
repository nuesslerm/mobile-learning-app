import {
  act,
  fireEvent,
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { useTamilProgressStore } from '../../state/tamilProgressStore';

describe('Tamil writing course flow', () => {
  beforeEach(() => {
    useTamilProgressStore.setState({ characters: {}, totalAttempts: 0 });
  });

  it('given the course catalogue, when the list renders, then every lesson gets a row with its Tamil title', async () => {
    await app.render('/tamil/alphabet');

    await waitFor(() => {
      expect(screen.getByTestId('TamilCourseScreen')).toBeTruthy();
    });

    // Six base lessons plus one per consonant: the whole உயிர்மெய் grid.
    const rows = screen.getAllByTestId('TamilLessonRow');
    expect(rows).toHaveLength(24);

    const sections = screen.getAllByTestId('TamilCourseSectionTitle');
    expect(sections.map((section) => section.textContent)).toEqual([
      'உயிர் · Vowels',
      'மெய் · Consonants',
      'ஆய்தம் · Other letters',
      'உயிர்மெய் · Combinations',
    ]);

    const firstTitle = screen.getAllByTestId('TamilLessonRowTitle')[0];
    expect(firstTitle.textContent).toContain('உயிர் எழுத்து');
  });

  it('given a lesson row, when tapped, then the app pushes that lesson route', async () => {
    const router = routerHarness();
    await app.render('/tamil/alphabet');

    await waitFor(() => {
      expect(screen.getAllByTestId('TamilLessonRow').length).toBeGreaterThan(0);
    });

    await userEvent.click(screen.getAllByTestId('TamilLessonRow')[0]);

    await waitFor(() => {
      expect(router.lastPush()).toBe('/tamil/uyir-1');
    });
  });

  it('given the vowel lesson, when the practice screen renders, then the first letter and its guide glyph are shown', async () => {
    await app.render('/tamil/uyir-1');

    await waitFor(() => {
      expect(screen.getByTestId('TamilLessonScreen')).toBeTruthy();
    });

    expect(screen.getByTestId('TamilPracticeGlyph').textContent).toBe('அ');
    expect(screen.getByTestId('TamilPracticePosition').textContent).toContain('1 of 6');
  });

  it('given a letter with authored stroke data, when the practice screen renders, then it asks for the first stroke and names what the hand does', async () => {
    await app.render('/tamil/uyir-1');

    await waitFor(() => {
      expect(screen.getByTestId('TamilPracticeStrokeCoach')).toBeTruthy();
    });

    expect(screen.getByTestId('TamilPracticeStrokeStep').textContent).toMatch(
      /^Stroke 1 of \d+$/
    );
    // The hint is read off the stroke it describes, so it always opens by
    // saying where the stroke begins.
    expect(
      screen.getByTestId('TamilPracticeStrokeGuideHint').textContent
    ).toMatch(/^From the /);
    // The printed letter is the font's own outline, filled -- the same
    // geometry the reference stroke was thinned out of, and the same one the
    // validator marks against.
    expect(screen.getByTestId('TamilPracticeCanvasOutline')).toBeTruthy();
    expect(screen.getByTestId('TamilPracticeCanvasActiveGuide')).toBeTruthy();
    expect(screen.getByTestId('TamilPracticeCanvasStartDot')).toBeTruthy();
    expect(screen.getByTestId('TamilPracticeCanvasDirectionArrow')).toBeTruthy();
  });

  it('given a letter with no authored stroke data, when the practice screen renders, then it falls back to free tracing', async () => {
    await app.render('/tamil/vallinam');

    await waitFor(() => {
      expect(screen.getByTestId('TamilLessonScreen')).toBeTruthy();
    });

    expect(screen.queryByTestId('TamilPracticeStrokeCoach')).toBeNull();
    expect(screen.queryByTestId('TamilPracticeCanvasOutline')).toBeNull();
    expect(screen.getByTestId('TamilPracticeCanvasGuideGlyph').textContent).toBe('க்');
    expect(screen.getByTestId('TamilPracticeFeedback').textContent).toContain(
      'Trace over the guide letter'
    );
  });

  it('given a letter on the pad, when graded Got it, then its mastery is recorded and a different letter is shown', async () => {
    await app.render('/tamil/uyir-1');

    await waitFor(() => {
      expect(screen.getByTestId('TamilPracticeGotIt')).toBeTruthy();
    });

    await userEvent.click(screen.getByTestId('TamilPracticeGotIt'));

    await waitFor(() => {
      expect(useTamilProgressStore.getState().masteryOf('uyir-a')).toBe(1);
      expect(screen.getByTestId('TamilPracticeGlyph').textContent).not.toBe('அ');
    });
  });

  it('given the second letter of one lesson, when a link opens another lesson on the same screen, then it starts at its first letter', async () => {
    const session = await app.render('/tamil/uyir-1');
    await waitFor(() => {
      expect(screen.getByTestId('TamilPracticeNext')).toBeTruthy();
    });
    await userEvent.click(screen.getByTestId('TamilPracticeNext'));
    await waitFor(() => {
      expect(screen.getByTestId('TamilPracticePosition').textContent).toContain('2 of 6');
    });

    // expo-router follows a link to the same route with new params by updating the mounted screen
    act(() => {
      routerHarness().setParams({ lessonId: 'uyirmei-k' });
    });

    await waitFor(() => {
      expect(screen.getByTestId('TamilPracticePosition').textContent).toContain('1 of 12');
    });

    await session.layout('TamilPracticeCanvas', { width: 320, height: 320 });
    await session.layout('TamilPracticeCanvasSurface', { width: 320, height: 320 });
    const surface = screen.getByTestId('TamilPracticeCanvasSurface');
    await waitFor(
      () => {
        expect(surface.getAttribute('data-active-guide')).not.toMatch(/^M 0 0 L 0 0/);
      },
      { timeout: 5000 }
    );
    expect(screen.getByTestId('TamilPracticeGlyph').textContent).toBe('க');
  });

  it('given an unknown lesson id, when the practice screen renders, then it says so instead of crashing', async () => {
    await app.render('/tamil/no-such-lesson');

    await waitFor(() => {
      expect(screen.getByTestId('TamilLessonMissing')).toBeTruthy();
    });
  });

  it('given the review deck, when a card is answered, then the piles move and the next card shows', async () => {
    await app.render('/tamil/review', { search: { lessonId: 'uyir-1' } });

    await waitFor(() => {
      expect(screen.getByTestId('TamilReviewScreen')).toBeTruthy();
    });

    // The prompt is what the letter is called, never the letter itself.
    const prompt = screen.getByTestId('TamilReviewPrompt').textContent;
    expect(prompt).toBeTruthy();
    expect(screen.queryByTestId('TamilReviewAnswerGlyph')).toBeNull();
    expect(screen.getByTestId('TamilReviewKnownCount').textContent).toContain('0 GOT IT');

    await userEvent.click(screen.getByTestId('TamilReviewGotIt'));

    await waitFor(() => {
      expect(screen.getByTestId('TamilReviewKnownCount').textContent).toContain('1 GOT IT');
    });
    expect(screen.getByTestId('TamilReviewPrompt').textContent).not.toBe(prompt);
    expect(useTamilProgressStore.getState().totalAttempts).toBe(1);
  });

  it('given a card the learner cannot write, when the letter is revealed, then it is shown with its name', async () => {
    await app.render('/tamil/review', { search: { lessonId: 'uyir-1' } });

    await waitFor(() => {
      expect(screen.getByTestId('TamilReviewScreen')).toBeTruthy();
    });

    await userEvent.click(screen.getByTestId('TamilReviewReveal'));

    await waitFor(() => {
      expect(screen.getByTestId('TamilReviewAnswerGlyph')).toBeTruthy();
    });
    expect(screen.getByTestId('TamilReviewAnswerName').textContent).toBeTruthy();

    // Missing it puts it in the other pile, and the answer hides again.
    await userEvent.click(screen.getByTestId('TamilReviewAgain'));
    await waitFor(() => {
      expect(screen.getByTestId('TamilReviewLearningCount').textContent).toContain('1 AGAIN');
    });
    expect(screen.queryByTestId('TamilReviewAnswerGlyph')).toBeNull();
  });

  it('Given a guided character, When its pad opens, Then it shows a clean glyph and lets the learner opt into guidance', async () => {
    await app.render('/tamil/write/uyir-a');

    await waitFor(() => screen.getByTestId('TamilWriteCanvasGuideGlyph'));
    expect(screen.queryByText('Check strokes')).toBeNull();
    expect(screen.getByTestId('TamilWriteDone').getAttribute('aria-disabled')).toBe('true');

    await userEvent.click(screen.getByLabelText('Toggle stroke guide'));
    await waitFor(() => screen.getByText('Check strokes'));
  });

  it('Given a word to write, When its pad renders, Then it shows the bundled-font model', async () => {
    await app.render('/tamil/write/word-uyir-i');

    const glyph = await waitFor(() => screen.getByTestId('TamilWriteCanvasGuideGlyph'));
    expect(glyph.textContent).toBe('இலை');
    expect(glyph.className).toContain('font_tamil');
  });

  it('Given a pure consonant pad with ink, When the learner self-checks, Then local mastery records the attempt once', async () => {
    await app.render('/tamil/write/mei-m');
    const surface = await waitFor(() => screen.getByTestId('TamilWriteCanvasSurface'));
    fireEvent.pointerDown(surface, { clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(surface, { clientX: 40, clientY: 40, pointerId: 1 });
    fireEvent.pointerUp(surface, { clientX: 40, clientY: 40, pointerId: 1 });
    await waitFor(() => expect(screen.getByTestId('TamilWriteHint').textContent).toBe('1 stroke'));

    await userEvent.click(screen.getByTestId('TamilWriteDone'));
    await waitFor(() => expect(screen.getByTestId('TamilWriteVerdict').textContent).toBe('Practice complete'));
    expect(useTamilProgressStore.getState().characters['mei-m']?.attempts).toBe(1);
  });

  it('given ink on one write pad, when a link opens another subject on the same screen, then its pad starts empty', async () => {
    await app.render('/tamil/write/uyir-a');
    await waitFor(() => {
      expect(screen.getByTestId('TamilWriteCanvasSurface')).toBeTruthy();
    });
    const surface = screen.getByTestId('TamilWriteCanvasSurface');
    fireEvent.pointerDown(surface, { clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(surface, { clientX: 40, clientY: 40, pointerId: 1 });
    fireEvent.pointerUp(surface, { clientX: 40, clientY: 40, pointerId: 1 });
    await waitFor(() => {
      expect(screen.getByTestId('TamilWriteHint').textContent).toBe('1 stroke');
    });

    // expo-router follows a link to the same route with new params by updating the mounted screen
    act(() => {
      routerHarness().setParams({ subjectId: 'word-uyir-a' });
    });

    await waitFor(() => {
      expect(screen.getByTestId('TamilWriteHint').textContent).toBe('Write it as large as you like.');
    });
  });

  it('given the write pad, when a stroke is drawn and released, then React reports no update of the screen while the pad renders', async () => {
    // oxlint-disable-next-line no-restricted-properties -- spy on console to assert React logs no update warning, not a project module
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await app.render('/tamil/write/uyir-a');
    await waitFor(() => {
      expect(screen.getByTestId('TamilWriteCanvasSurface')).toBeTruthy();
    });

    const surface = screen.getByTestId('TamilWriteCanvasSurface');
    fireEvent.pointerDown(surface, { clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(surface, { clientX: 40, clientY: 40, pointerId: 1 });
    fireEvent.pointerUp(surface, { clientX: 40, clientY: 40, pointerId: 1 });

    await waitFor(() => {
      expect(screen.getByTestId('TamilWriteDone')).toBeTruthy();
    });
    const reported = errors.mock.calls.map((call) => String(call[0]));
    errors.mockRestore();
    expect(reported.filter((m) => m.includes('Cannot update a component'))).toEqual([]);
  });
});