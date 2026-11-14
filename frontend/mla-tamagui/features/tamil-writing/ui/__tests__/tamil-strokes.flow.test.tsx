import { beforeEach, describe, expect, it } from 'vitest';
import type { FlowSession, StrokeTransform } from '@lib/flow-test-harness';

type GuidePoint = { x: number; y: number };

/**
 * The reference path the app is currently showing, as fractions of the pad,
 * read off the page the same way the browser check does: the surface's
 * data-active-guide path, each command's last two numbers, divided by the
 * surface box.
 */
async function activeGuidePoints(session: FlowSession): Promise<GuidePoint[]> {
  const { waitFor } = await import('@lib/flow-test-harness');
  let points: GuidePoint[] = [];
  await waitFor(
    () => {
      const surface = session.getByTestId('TamilPracticeCanvasSurface');
      const box = surface.getBoundingClientRect();
      points = (surface.getAttribute('data-active-guide') ?? '')
        .split(/(?=[MQL])/)
        .map((piece) =>
          piece.trim().split(' ').filter(Boolean).slice(1).map(Number)
        )
        .filter((nums) => nums.length >= 2 && nums.every((n) => Number.isFinite(n)))
        .map((nums) => nums.slice(-2))
        .map(([x, y]) => ({ x: x / box.width, y: y / box.height }));
      expect(points.some((p) => p.x !== 0 || p.y !== 0)).toBe(true);
      expect(points.length).toBeGreaterThan(1);
    },
    { timeout: 5000 }
  );
  return points;
}

describe('Tamil practice strokes', () => {
  beforeEach(async () => {
    const { useTamilProgressStore } = await import(
      '../../state/tamilProgressStore'
    );
    useTamilProgressStore.setState({ characters: {}, totalAttempts: 0 });
  });

  it('given the uyir-a pad, when each wrong stroke is drawn, then it is refused with its own reason and leaves no ink', async () => {
    const { app } = await import('../../../../tests/flow/app');
    const session = await app.render('/tamil/uyir-1');
    await import('@lib/flow-test-harness').then(({ waitFor }) =>
      waitFor(() => {
        expect(session.getByTestId('TamilPracticeStrokeCoach')).toBeTruthy();
      })
    );
    // The canvas size reaches TracingCanvas through its own onLayout; the
    // surface's box is what the pointer samples are measured against.
    await session.layout('TamilPracticeCanvas', { width: 320, height: 320 });
    await session.layout('TamilPracticeCanvasSurface', { width: 320, height: 320 });

    const reference = await activeGuidePoints(session);
    const firstStep = session.getByTestId('TamilPracticeStrokeStep').textContent;

    await session.snapshot('pad-empty');

    const wrong: [string, StrokeTransform, string][] = [
      ['backwards', { reverse: true }, 'wrong way'],
      ['displaced', { offset: { x: -0.3, y: 0.1 } }, 'marked dot'],
      ['cut short', { truncate: 0.25 }, 'Too short'],
    ];

    let refusalIndex = 0;
    for (const [_label, transform, expected] of wrong) {
      await session.stroke('TamilPracticeCanvasSurface', reference, {
        samplesPerSegment: 8,
        transform,
      });

      expect(
        session.getByTestId('TamilPracticeStrokeStep').textContent
      ).toBe(firstStep);
      expect(
        session.getByTestId('TamilPracticeFeedback').textContent
      ).toContain(expected);
      const surface = session.getByTestId('TamilPracticeCanvasSurface');
      expect(surface.getAttribute('data-ink-count')).toBe('0');

      if (refusalIndex === 0) {
        await session.snapshot('stroke-refused-backwards');
      }
      refusalIndex += 1;
    }
  });

  it('given the reference stroke, when it is written, then it is accepted and the pad advances', async () => {
    const { app } = await import('../../../../tests/flow/app');
    const session = await app.render('/tamil/uyir-1');
    await import('@lib/flow-test-harness').then(({ waitFor }) =>
      waitFor(() => {
        expect(session.getByTestId('TamilPracticeStrokeCoach')).toBeTruthy();
      })
    );
    await session.layout('TamilPracticeCanvas', { width: 320, height: 320 });
    await session.layout('TamilPracticeCanvasSurface', { width: 320, height: 320 });

    const reference = await activeGuidePoints(session);
    const firstStep = session.getByTestId('TamilPracticeStrokeStep').textContent;

    await session.stroke('TamilPracticeCanvasSurface', reference, {
      samplesPerSegment: 8,
    });

    expect(
      session.getByTestId('TamilPracticeStrokeStep').textContent
    ).not.toBe(firstStep);
    const surface = session.getByTestId('TamilPracticeCanvasSurface');
    expect(surface.getAttribute('data-ink-count')).toBe('1');

    await session.snapshot('stroke-accepted');
  });

  it('given every stroke of the letter, when they are written in order, then the letter is finished and scored', async () => {
    const { app } = await import('../../../../tests/flow/app');
    const session = await app.render('/tamil/uyir-1');
    await import('@lib/flow-test-harness').then(({ waitFor }) =>
      waitFor(() => {
        expect(session.getByTestId('TamilPracticeStrokeCoach')).toBeTruthy();
      })
    );
    await session.layout('TamilPracticeCanvas', { width: 320, height: 320 });
    await session.layout('TamilPracticeCanvasSurface', { width: 320, height: 320 });

    const strokeCount = Number(
      /Stroke 1 of (\d+)/.exec(
        session.getByTestId('TamilPracticeStrokeStep').textContent ?? ''
      )?.[1]
    );
    expect(strokeCount).toBeGreaterThan(0);

    for (let stroke = 1; stroke <= strokeCount; stroke += 1) {
      const reference = await activeGuidePoints(session);
      await session.stroke('TamilPracticeCanvasSurface', reference, {
        samplesPerSegment: 8,
      });
    }

    expect(
      session.getByTestId('TamilPracticeStrokeStep').textContent
    ).toMatch(new RegExp(`^All ${strokeCount} stroke`));
    expect(
      session.getByTestId('TamilPracticeStrokeScore').textContent
    ).toContain('% ON THE LINE');

    await session.snapshot('letter-finished');
  });
});