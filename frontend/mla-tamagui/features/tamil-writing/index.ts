/**
 * Public barrel for the Tamil handwriting course. Routes import from here
 * only; nothing outside this directory reaches into its layers.
 *
 * Same shape as the leetcode feature next door: `domain/` is pure (letters,
 * mastery ladder, stroke geometry), `data/` is the catalogue facade, `state/`
 * is the persisted progress store, `ui/` is screens.
 */

// Screens (ui layer)
export { TamilCourseScreen } from './ui/TamilCourseScreen';
export { TamilLessonScreen } from './ui/TamilLessonScreen';
export { TamilReviewScreen } from './ui/TamilReviewScreen';
export { TamilWriteScreen } from './ui/TamilWriteScreen';
export { TamilTracksScreen } from './ui/TamilTracksScreen';
export { TamilTrackScreen } from './ui/TamilTrackScreen';

export { tamilPageParams } from './data/pageParams';
