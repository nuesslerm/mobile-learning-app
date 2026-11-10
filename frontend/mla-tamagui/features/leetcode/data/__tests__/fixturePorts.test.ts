import { afterEach, describe, expect, it, vi } from 'vitest';

import { createFixturePorts } from '../fixturePorts';

describe('createFixturePorts', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('Given unit fixture ports, When userStatus is called, Then it resolves signed in as fixture-user', async () => {
    const ports = createFixturePorts();

    await expect(ports.gateway.userStatus()).resolves.toEqual({
      isSignedIn: true,
      username: 'fixture-user',
    });
  });

  it('Given unit fixture ports, When plan is called, Then it resolves two alternatives and the second renames nums to values', async () => {
    const ports = createFixturePorts();

    const plan = await ports.voice.plan({
      transcript: 'rename nums to values and return early when target is found',
      currentCode:
        'function twoSum(nums: number[], target: number): number[] {\n    \n};',
      language: 'typescript',
    });

    expect(plan.alternatives).toHaveLength(2);
    const second = plan.alternatives[1];
    expect(second.id).toBe('rename-to-values');
    expect(
      second.commands.some(
        (c) =>
          c.type === 'replaceLines' &&
          c.newText.includes('values') &&
          !c.newText.includes('nums')
      )
    ).toBe(true);
  });

  it('Given a started unit fixture recording, When levels are emitted, Then subscribers observe them and stop returns the pinned recording', async () => {
    vi.useFakeTimers();
    const ports = createFixturePorts();
    const seen: { isRecording: boolean; durationMs: number; level: number }[] =
      [];
    ports.audio.subscribe((sample) => seen.push(sample));

    await ports.audio.start();
    expect(seen[0]).toEqual({ isRecording: true, durationMs: 0, level: 0.5 });

    vi.advanceTimersByTime(160);
    expect(seen.map((s) => s.level)).toEqual([0.5, 0.62, 0.73]);

    await expect(ports.audio.stop()).resolves.toEqual({
      uri: 'fixture://take.m4a',
      mimeType: 'audio/m4a',
      durationMs: 2400,
    });
    expect(seen.at(-1)).toEqual({ isRecording: false, durationMs: 2400, level: 0 });
  });
});