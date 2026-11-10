import type { TamilCharacter } from '../domain/characters';
import {
  TAMIL_CHARACTERS,
  TAMIL_LESSONS,
  getCharacter,
  getLesson,
  getLessonCharacters,
  lessonsBySection,
  type LessonGroup,
  type TamilLesson,
} from '../domain/lessons';

/**
 * The course, as the UI sees it. Mirrors `ProblemCatalog`'s shape so the two
 * features read the same: a frozen in-app catalogue behind a namespace, no
 * network and no loading state.
 */
export const TamilCourse = {
  getLessons(): readonly TamilLesson[] {
    return TAMIL_LESSONS;
  },

  /** Lessons grouped by section -- what the course screen renders. */
  getLessonGroups(): LessonGroup[] {
    return lessonsBySection();
  },

  getLesson(lessonId: string): TamilLesson | undefined {
    return getLesson(lessonId);
  },

  getLessonCharacters(lessonId: string): TamilCharacter[] {
    return getLessonCharacters(lessonId);
  },

  getCharacter(characterId: string): TamilCharacter | undefined {
    return getCharacter(characterId);
  },

  getAllCharacters(): readonly TamilCharacter[] {
    return TAMIL_CHARACTERS;
  },

  getAllCharacterIds(): string[] {
    return TAMIL_CHARACTERS.map((character) => character.id);
  },
} as const;
