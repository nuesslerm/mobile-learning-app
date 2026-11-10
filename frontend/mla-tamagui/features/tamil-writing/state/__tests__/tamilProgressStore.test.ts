import { beforeEach, describe, expect, it, vi } from 'vitest';

import { MASTERY_MASTERED } from '../../domain/mastery';
import { useTamilProgressStore } from '../tamilProgressStore';

// Same shim as the leetcode auth store's suite: `persist` writes to
// AsyncStorage on every setState, and the real package's web shim reaches for
// `window`, which the `node` environment does not define.
vi.mock('@react-native-async-storage/async-storage', () => {
  const store = new Map<string, string>();
  return {
    default: {
      getItem: async (key: string) => store.get(key) ?? null,
      setItem: async (key: string, value: string) => {
        store.set(key, value);
      },
      removeItem: async (key: string) => {
        store.delete(key);
      },
    },
  };
});

describe('useTamilProgressStore', () => {
  beforeEach(() => {
    useTamilProgressStore.setState({ characters: {}, totalAttempts: 0 });
  });

  it('Given an unpractised letter, When an attempt is graded got-it, Then its rung, attempts and timestamp are recorded', () => {
    useTamilProgressStore.getState().recordAttempt('uyir-a', 'got-it');

    const progress = useTamilProgressStore.getState().characters['uyir-a'];
    expect(progress.mastery).toBe(1);
    expect(progress.attempts).toBe(1);
    expect(progress.lastPracticedAt).toBeGreaterThan(0);
    expect(useTamilProgressStore.getState().totalAttempts).toBe(1);
  });

  it('Given three got-it attempts, When mastery is read, Then the letter is mastered and stays there', () => {
    const { recordAttempt } = useTamilProgressStore.getState();
    for (let i = 0; i < 5; i += 1) recordAttempt('uyir-a', 'got-it');

    expect(useTamilProgressStore.getState().masteryOf('uyir-a')).toBe(MASTERY_MASTERED);
    expect(useTamilProgressStore.getState().characters['uyir-a'].attempts).toBe(5);
  });

  it('Given a mastered letter, When an attempt is graded again, Then it drops one rung but keeps its attempt count', () => {
    const { recordAttempt } = useTamilProgressStore.getState();
    recordAttempt('mei-k', 'got-it');
    recordAttempt('mei-k', 'got-it');
    recordAttempt('mei-k', 'again');

    const progress = useTamilProgressStore.getState().characters['mei-k'];
    expect(progress.mastery).toBe(1);
    expect(progress.attempts).toBe(3);
  });

  it('Given an empty character id, When an attempt is recorded, Then nothing is written', () => {
    useTamilProgressStore.getState().recordAttempt('', 'got-it');

    expect(useTamilProgressStore.getState().characters).toEqual({});
    expect(useTamilProgressStore.getState().totalAttempts).toBe(0);
  });

  it('Given an unpractised letter, When its mastery is read, Then it reads as zero rather than undefined', () => {
    expect(useTamilProgressStore.getState().masteryOf('uyir-oo')).toBe(0);
  });

  it('Given practice across two lessons, When one lesson is reset, Then only that lesson loses its progress', () => {
    const { recordAttempt } = useTamilProgressStore.getState();
    recordAttempt('uyir-a', 'got-it');
    recordAttempt('mei-k', 'got-it');

    useTamilProgressStore.getState().resetLesson(['uyir-a']);

    const state = useTamilProgressStore.getState();
    expect(state.characters['uyir-a']).toBeUndefined();
    expect(state.characters['mei-k'].mastery).toBe(1);
  });

  it('Given recorded progress, When everything is reset, Then the store is empty again', () => {
    useTamilProgressStore.getState().recordAttempt('uyir-a', 'got-it');
    useTamilProgressStore.getState().resetAll();

    const state = useTamilProgressStore.getState();
    expect(state.characters).toEqual({});
    expect(state.totalAttempts).toBe(0);
  });

  it('Given several practised letters, When the mastery map is read, Then it is a plain id-to-rung record', () => {
    const { recordAttempt } = useTamilProgressStore.getState();
    recordAttempt('uyir-a', 'got-it');
    recordAttempt('uyir-i', 'got-it');
    recordAttempt('uyir-i', 'got-it');

    expect(useTamilProgressStore.getState().masteryMap()).toEqual({
      'uyir-a': 1,
      'uyir-i': 2,
    });
  });
});
