import { TRACKS } from '../domain/tracks';

import { TamilCourse } from './tamilCourse';

/** The ids the web export writes a page for (the routes' `generateStaticParams`); an unlisted id gets the template page. */
export const tamilPageParams = {
  lessons: () => TamilCourse.getLessons().map(({ id }) => ({ lessonId: id })),
  tracks: () => TRACKS.map(({ id }) => ({ trackId: id })),
  subjects: () => {
    const ids = new Set(TRACKS.flatMap((track) => track.subjects.map((subject) => subject.id)));
    return [...ids].map((subjectId) => ({ subjectId }));
  },
};
