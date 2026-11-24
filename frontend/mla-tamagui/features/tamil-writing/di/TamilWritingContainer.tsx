import { overrideKey, useDependencyOverrides } from '@lib/app-di';
import { useEffect, useMemo, type ReactNode } from 'react';
import { createJSONStorage } from 'zustand/middleware';

import { createExpoTamilAudio } from '../data/lessons/audio';
import { curriculum } from '../data/lessons/curriculum';
import { createCourseProgress } from '../data/progress/courseProgress';
import { createDeviceStorage } from '../data/progress/deviceStorage';
import { createSnapshotStorage } from '../data/progress/snapshotStorage';
import { createWritingStorage } from '../data/progress/writingStorage';
import type { TamilWritingPorts } from '../domain/ports';
import { TamilWritingPortsProvider } from '../state/ports';
import { useTamilProgressStore } from '../state/tamilProgressStore';

export const TAMIL_WRITING_OVERRIDES = overrideKey<TamilWritingPorts>('tamilWriting');

const storage = createSnapshotStorage(createDeviceStorage());
const courseProgress = createCourseProgress(storage);

useTamilProgressStore.persist.setOptions({ storage: createJSONStorage(() => createWritingStorage(storage)) });
let initialized = false;

export function createDefaultPorts(overrides: Partial<TamilWritingPorts> = {}): TamilWritingPorts {
  return {
    curriculum: overrides.curriculum ?? curriculum,
    courseProgress: overrides.courseProgress ?? courseProgress,
    audio: overrides.audio ?? createExpoTamilAudio(),
  };
}

export function TamilWritingContainer({ children }: { children: ReactNode }) {
  const overrides = useDependencyOverrides(TAMIL_WRITING_OVERRIDES);
  useEffect(() => {
    if (initialized) return;
    initialized = true;
    void useTamilProgressStore.persist.rehydrate();
  }, []);
  const ports = useMemo(() => createDefaultPorts(overrides), [overrides]);
  return <TamilWritingPortsProvider value={ports}>{children}</TamilWritingPortsProvider>;
}
