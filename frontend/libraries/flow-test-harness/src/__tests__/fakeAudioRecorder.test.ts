import { describe, expect, it, vi } from 'vitest';

import { createFakeAudioRecorder } from '../doubles/fakeAudioRecorder';

describe('createFakeAudioRecorder', () => {
  it('given a granted recorder, when start, emitLevel and stop run, then subscribers observe the lifecycle and stop returns the recording', async () => {
    const recorder = createFakeAudioRecorder({
      levels: [0.2, 0.8],
      durationMs: 3400,
      permission: true,
    });
    const snapshots = vi.fn();
    const unsubscribe = recorder.subscribe(snapshots);

    await expect(recorder.requestPermission()).resolves.toBe(true);

    await recorder.start();
    expect(snapshots).toHaveBeenLastCalledWith({
      isRecording: true,
      durationMs: 3400,
      level: 0.8,
    });

    recorder.emitLevel(0.5);
    expect(snapshots).toHaveBeenLastCalledWith({
      isRecording: true,
      durationMs: 3400,
      level: 0.5,
    });

    await expect(recorder.stop()).resolves.toEqual({
      uri: 'fake://recording.m4a',
      mimeType: 'audio/m4a',
      durationMs: 3400,
    });
    expect(snapshots).toHaveBeenLastCalledWith({
      isRecording: false,
      durationMs: 3400,
      level: 0.5,
    });
    expect(recorder.calls).toEqual([
      'requestPermission',
      'start',
      'emitLevel',
      'stop',
    ]);

    unsubscribe();
    recorder.emitLevel(0.1);
    expect(snapshots).toHaveBeenCalledTimes(5);
  });

  it('given denied permission, when requestPermission and start run, then permission resolves false and start rejects without pushing', async () => {
    const recorder = createFakeAudioRecorder({ permission: false });
    const snapshots = vi.fn();
    recorder.subscribe(snapshots);

    await expect(recorder.requestPermission()).resolves.toBe(false);
    await expect(recorder.start()).rejects.toThrow('permission denied');
    expect(snapshots).not.toHaveBeenCalled();
    expect(recorder.calls).toEqual(['requestPermission', 'start']);
  });

  it('given a stopped recorder, when stop runs again, then null is returned', async () => {
    const recorder = createFakeAudioRecorder();
    await recorder.start();
    await recorder.stop();
    await expect(recorder.stop()).resolves.toBeNull();
  });

  it('given a recorder never asked, when it is asked and the person allows, then the permission moves to granted and recording works', async () => {
    const recorder = createFakeAudioRecorder({ permission: 'prompt' });
    expect(recorder.permission).toBe('prompt');

    await expect(recorder.start()).rejects.toThrow('never requested');
    await expect(recorder.requestPermission()).resolves.toBe(true);

    expect(recorder.permission).toBe('granted');
    await expect(recorder.start()).resolves.toBeUndefined();
  });

  it('given a recorder never asked, when it is asked and the person refuses, then the permission moves to denied and recording is refused', async () => {
    const recorder = createFakeAudioRecorder({ permission: 'prompt', promptAnswer: false });

    await expect(recorder.requestPermission()).resolves.toBe(false);

    expect(recorder.permission).toBe('denied');
    await expect(recorder.start()).rejects.toThrow('denied');
  });
});
