import { curriculum } from '../data/lessons/curriculum';
import { characterGroups } from '../domain/handwriting/catalog';
import { TRACKS } from '../domain/tracks';

import { TamilCourse } from '../domain/course';

/** The ids the web export writes a page for (the routes' `generateStaticParams`); an unlisted id gets the template page. */
export const tamilPageParams = {
  topics: async () => (await curriculum.listSections()).flatMap((section) => section.topics.map(({ id }) => ({ groupId: id }))),
  tasks: async () => (await curriculum.listSections()).flatMap((section) =>
    section.topics.flatMap((topic) => topic.tasks.map((task) => ({ groupId: topic.id, taskId: task.id })))
  ),
  lessons: () => TamilCourse.getLessons().map(({ id }) => ({ lessonId: id })),
  tracks: () => TRACKS.map(({ id }) => ({ trackId: id })),
  subjects: () => {
    const ids = new Set(TRACKS.flatMap((track) => track.subjects.map((subject) => subject.id)));
    for (const group of characterGroups) for (const character of group.characters) ids.add(character.id);
    return [...ids].map((subjectId) => ({ subjectId }));
  },
};
