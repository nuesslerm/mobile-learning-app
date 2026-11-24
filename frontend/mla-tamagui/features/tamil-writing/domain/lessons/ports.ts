import type { Answer, CourseProgress, CourseUpdate, Section, Verdict } from './course';

export type CurriculumPort = {
  listSections(): Promise<readonly Section[]>;
  check(taskId: string, exerciseId: string, answer: Answer): Promise<Verdict>;
};

export type CourseProgressPort = {
  read(): Promise<CourseProgress>;
  update(update: CourseUpdate): Promise<CourseProgress>;
};

export type TamilAudioPort = {
  play(exerciseId: string, slow: boolean): Promise<void>;
  stop(): void;
};
