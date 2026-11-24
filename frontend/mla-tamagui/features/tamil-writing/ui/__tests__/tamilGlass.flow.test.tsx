import {
  routerHarness,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
  within,
} from '@lib/flow-test-harness';
import { Platform } from 'react-native';
import { afterEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';

const realOS = Platform.OS;

afterEach(() => {
  (Platform as { OS: string }).OS = realOS;
});

function nativeBar() {
  (Platform as { OS: string }).OS = 'ios';
  setStackHeadersRendered(true);
}

const screens = [
  ['/tamil/uyir-1', 'TamilLessonHeader'],
  ['/tamil/no-such-lesson', 'TamilLessonMissingHeader'],
  ['/tamil/review', 'TamilReviewHeader'],
  ['/tamil/tracks', 'TamilTracksHeader'],
  ['/tamil/tracks/no-such-track', 'TamilTrackMissingHeader'],
  ['/tamil/write/uyir-a', 'TamilWriteHeader'],
  ['/tamil/write/no-such-subject', 'TamilWriteMissingHeader'],
] as const;

describe('Tamil screens on the native header', () => {
  it.each(screens)(
    'Given %s on the native bar, When the back item is pressed, Then the router went back and the classic header is absent',
    async (route, headerId) => {
      // Given the native bar
      nativeBar();
      await app.render(route);
      await waitFor(() => screen.getByTestId('NativeHeaderItem-Go back'));
      expect(screen.queryByTestId(headerId)).toBeNull();

      // When the back item is pressed
      await userEvent.click(screen.getByTestId('NativeHeaderItem-Go back'));

      // Then the router went back
      expect(routerHarness().calls()).toContainEqual(['back']);
    }
  );

  it('Given the lesson on the native bar and its second letter, When the reset action is pressed, Then the lesson starts again at its first letter', async () => {
    // Given the second letter of a lesson
    nativeBar();
    await app.render('/tamil/uyir-1');
    await waitFor(() => screen.getByTestId('TamilPracticeNext'));
    await userEvent.click(screen.getByTestId('TamilPracticeNext'));
    await waitFor(() =>
      expect(screen.getByTestId('TamilPracticePosition').textContent).toContain(
        '2 of 6'
      )
    );

    // When the reset action is pressed
    await userEvent.click(
      screen.getByTestId('NativeHeaderItem-Reset this lesson')
    );

    // Then the lesson is at its first letter
    await waitFor(() =>
      expect(screen.getByTestId('TamilPracticePosition').textContent).toContain(
        '1 of 6'
      )
    );
  });

  it('Given the write screen on the native bar, When it mounts, Then the capsule holds the glyph and transliteration and the label', async () => {
    // Given the native bar
    nativeBar();

    // When the write screen mounts
    await app.render('/tamil/write/uyir-a');
    await waitFor(() => screen.getByTestId('ScreenHeaderGlassTitle'));

    // Then the title carries the glyph and its transliteration
    expect(
      within(screen.getByTestId('ScreenHeaderGlassTitle')).getByText(/அ · /)
    ).not.toBeNull();
  });

  it('Given the missing lesson on the native bar, When it mounts, Then the title says so', async () => {
    // Given the native bar
    nativeBar();

    // When an unknown lesson mounts
    await app.render('/tamil/no-such-lesson');

    // Then the capsule keeps the missing title
    await waitFor(() =>
      expect(
        within(screen.getByTestId('ScreenHeaderGlassTitle')).getByText(
          'Lesson not found'
        )
      ).not.toBeNull()
    );
  });
});
