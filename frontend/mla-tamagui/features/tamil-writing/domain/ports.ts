import type { FlashCardProgressPort } from './lessons/flashCards';
import type { CourseProgressPort, CurriculumPort, TamilAudioPort } from './lessons/ports';

export type TamilWritingPorts = {
  flashCards: FlashCardProgressPort;
  curriculum: CurriculumPort;
  courseProgress: CourseProgressPort;
  audio: TamilAudioPort;
};
