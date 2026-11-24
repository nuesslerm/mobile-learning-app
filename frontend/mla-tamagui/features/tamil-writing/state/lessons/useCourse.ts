import { Logger } from '@lib/logging';
import { useCallback, useEffect, useState } from 'react';

import type { CourseProgress, CourseUpdate, Section } from '../../domain/lessons/course';
import { useTamilWritingPorts } from '../ports';

type Course = { sections: readonly Section[]; progress: CourseProgress };

export function useCourse() {
  const { curriculum, courseProgress } = useTamilWritingPorts();
  const [course, setCourse] = useState<Course>();
  const [error, setError] = useState(false);

  const reload = useCallback(async () => {
    try {
      const [sections, progress] = await Promise.all([curriculum.listSections(), courseProgress.read()]);
      setCourse({ sections, progress });
      setError(false);
    } catch (cause) {
      Logger.withError(cause).error('tamil.course.load_failed');
      setError(true);
    }
  }, [curriculum, courseProgress]);

  useEffect(() => { void reload(); }, [reload]);

  const save = useCallback(async (update: CourseUpdate) => {
    const progress = await courseProgress.update(update);
    setCourse((current) => current && { ...current, progress });
    return progress;
  }, [courseProgress]);

  return { course, error, reload, save };
}
