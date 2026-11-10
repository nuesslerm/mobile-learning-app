import { describe, it, expect, beforeEach, vi } from 'vitest';

import { usePlanStore } from '../planStore';

// zustand's `persist` middleware fire-and-forgets a write to AsyncStorage on
// every setState call; stub it with an in-memory map so store logic (and the
// persisted JSON itself) can be asserted without a storage backend. The map
// lives in `vi.hoisted` so it survives `vi.resetModules()` in the round-trip
// test, where the store module is re-imported fresh.
const storageMap = vi.hoisted(() => new Map<string, string>());

vi.mock('@react-native-async-storage/async-storage', () => {
  return {
    default: {
      getItem: async (key: string) => storageMap.get(key) ?? null,
      setItem: async (key: string, value: string) => {
        storageMap.set(key, value);
      },
      removeItem: async (key: string) => {
        storageMap.delete(key);
      },
    },
  };
});

const PROBLEM = 'two-sum';

function texts(problemId: string): string[] {
  return usePlanStore
    .getState()
    .stepsFor(problemId)
    .map((s) => s.text);
}

describe('usePlanStore', () => {
  beforeEach(() => {
    storageMap.clear();
    usePlanStore.setState({ plans: {} });
  });

  it('Given an empty plan, When addBlock receives a two-sentence block, Then it lands as one pending step', () => {
    usePlanStore
      .getState()
      .addBlock(PROBLEM, 'Sort the array. Scan with two pointers.');

    const steps = usePlanStore.getState().stepsFor(PROBLEM);
    expect(steps).toHaveLength(1);
    expect(steps[0].text).toBe('Sort the array. Scan with two pointers.');
    expect(steps[0].status).toBe('pending');
    expect(steps[0].lines).toBeNull();
  });

  it('Given a spoken block, When addBlock records its origin, Then the step remembers it was spoken', () => {
    usePlanStore.getState().addBlock(PROBLEM, 'Sort the array', 'voice');

    expect(usePlanStore.getState().stepsFor(PROBLEM)[0].origin).toBe('voice');
  });

  it('Given a plan with one step, When splitStep is called on a two-sentence step, Then the step is replaced by its sentences in place', () => {
    usePlanStore.getState().addBlock(PROBLEM, 'Build a prefix sum');
    const [step] = usePlanStore.getState().stepsFor(PROBLEM);

    usePlanStore.getState().splitStep(PROBLEM, step.id);

    expect(texts(PROBLEM)).toEqual(['Build a prefix sum']);
    usePlanStore.getState().addBlock(PROBLEM, 'Count pairs');
    usePlanStore.getState().addBlock(PROBLEM, 'Return the total');
    const first = usePlanStore.getState().stepsFor(PROBLEM)[1];
    usePlanStore.getState().splitStep(PROBLEM, first.id);

    const steps = usePlanStore.getState().stepsFor(PROBLEM);
    expect(steps.map((s) => s.text)).toEqual([
      'Build a prefix sum',
      'Count pairs',
      'Return the total',
    ]);
    expect(steps[1].id).toBe(first.id);
  });

  it('Given two adjacent steps, When mergeWithNext is called on the first, Then one step joins texts with a space and keeps the first id', () => {
    usePlanStore.getState().addBlock(PROBLEM, 'Sort the array');
    usePlanStore.getState().addBlock(PROBLEM, 'Scan with two pointers');
    const [first] = usePlanStore.getState().stepsFor(PROBLEM);

    usePlanStore.getState().mergeWithNext(PROBLEM, first.id);

    const steps = usePlanStore.getState().stepsFor(PROBLEM);
    expect(steps).toHaveLength(1);
    expect(steps[0].text).toBe('Sort the array Scan with two pointers');
    expect(steps[0].id).toBe(first.id);
  });

  it('Given three steps, When moveStep moves the middle step down and up, Then it swaps with its neighbour each time', () => {
    usePlanStore.getState().addBlock(PROBLEM, 'One');
    usePlanStore.getState().addBlock(PROBLEM, 'Two');
    usePlanStore.getState().addBlock(PROBLEM, 'Three');
    const [one, two] = usePlanStore.getState().stepsFor(PROBLEM);

    usePlanStore.getState().moveStep(PROBLEM, two.id, 1);
    expect(texts(PROBLEM)).toEqual(['One', 'Three', 'Two']);

    usePlanStore.getState().moveStep(PROBLEM, two.id, -1);
    expect(texts(PROBLEM)).toEqual(['One', 'Two', 'Three']);
    expect(usePlanStore.getState().stepsFor(PROBLEM)[0].id).toBe(one.id);
  });

  it('Given a plan, When moveStep would leave the bounds, Then the order is clamped and unchanged', () => {
    usePlanStore.getState().addBlock(PROBLEM, 'One');
    usePlanStore.getState().addBlock(PROBLEM, 'Two');
    const [first, second] = usePlanStore.getState().stepsFor(PROBLEM);

    usePlanStore.getState().moveStep(PROBLEM, first.id, -1);
    usePlanStore.getState().moveStep(PROBLEM, second.id, 1);

    expect(texts(PROBLEM)).toEqual(['One', 'Two']);
  });

  it('Given a plan with two steps, When removeStep is called, Then only the target step is removed', () => {
    usePlanStore.getState().addBlock(PROBLEM, 'One');
    usePlanStore.getState().addBlock(PROBLEM, 'Two');
    const [, second] = usePlanStore.getState().stepsFor(PROBLEM);

    usePlanStore.getState().removeStep(PROBLEM, second.id);

    expect(texts(PROBLEM)).toEqual(['One']);
  });

  it('Given a realized step, When markStaleFrom targets its index, Then the step turns stale but keeps its lines and other steps are untouched', () => {
    usePlanStore.getState().addBlock(PROBLEM, 'Sort the array');
    usePlanStore.getState().addBlock(PROBLEM, 'Scan with two pointers');
    const [first, second] = usePlanStore.getState().stepsFor(PROBLEM);
    usePlanStore
      .getState()
      .markRealized(PROBLEM, first.id, { startLine: 3, endLine: 5 });

    const realized = usePlanStore.getState().stepsFor(PROBLEM)[0];
    expect(realized.status).toBe('realized');
    expect(realized.lines).toEqual({ startLine: 3, endLine: 5 });

    usePlanStore.getState().markStaleFrom(PROBLEM, 0);

    const steps = usePlanStore.getState().stepsFor(PROBLEM);
    expect(steps[0].status).toBe('stale');
    expect(steps[0].lines).toEqual({ startLine: 3, endLine: 5 });
    expect(steps[1].status).toBe('pending');
    expect(steps[1].id).toBe(second.id);
  });

  it('Given two realized steps, When markStaleFrom starts after the first, Then only later realized steps go stale', () => {
    usePlanStore.getState().addBlock(PROBLEM, 'One');
    usePlanStore.getState().addBlock(PROBLEM, 'Two');
    usePlanStore.getState().addBlock(PROBLEM, 'Three');
    const [one, two] = usePlanStore.getState().stepsFor(PROBLEM);
    usePlanStore
      .getState()
      .markRealized(PROBLEM, one.id, { startLine: 1, endLine: 1 });
    usePlanStore
      .getState()
      .markRealized(PROBLEM, two.id, { startLine: 2, endLine: 2 });

    usePlanStore.getState().markStaleFrom(PROBLEM, 1);

    const steps = usePlanStore.getState().stepsFor(PROBLEM);
    expect(steps[0].status).toBe('realized');
    expect(steps[1].status).toBe('stale');
    expect(steps[2].status).toBe('pending');
  });

  it('Given a written plan, When the module is re-imported fresh against the same storage, Then steps rehydrate through the persisted JSON', async () => {
    usePlanStore.getState().addBlock(PROBLEM, 'Sort the array');
    usePlanStore.getState().addBlock(PROBLEM, 'Scan with two pointers');

    await vi.waitFor(() => {
      expect(storageMap.has('leetcode-plan')).toBe(true);
    });
    const raw = storageMap.get('leetcode-plan')!;
    const persisted = JSON.parse(raw);
    expect(persisted.version).toBe(1);
    expect(Object.keys(persisted.state.plans)).toContain(PROBLEM);

    vi.resetModules();
    const freshModule = await import('../planStore');
    await freshModule.usePlanStore.persist.rehydrate();

    const steps = freshModule.usePlanStore.getState().stepsFor(PROBLEM);
    expect(steps.map((s) => s.text)).toEqual([
      'Sort the array',
      'Scan with two pointers',
    ]);
    expect(steps.every((s) => s.status === 'pending' && s.lines === null)).toBe(
      true
    );
  });
});
