import type { CourseProgressPort, CurriculumPort, TamilAudioPort } from './lessons/ports';

export type TamilWritingPorts = {
  curriculum: CurriculumPort;
  courseProgress: CourseProgressPort;
  audio: TamilAudioPort;
};
