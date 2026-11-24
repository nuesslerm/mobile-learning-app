import allTopicsImage from './artwork/all-topics.png';
import dynamicProgrammingImage from './artwork/dynamic-programming.png';
import depthFirstImage from './artwork/depth-first.png';
import amazonImage from './artwork/amazon.png';
import appleImage from './artwork/apple.png';
import contestDetailImage from './artwork/contest-detail.jpg';
import contestImage from './artwork/contest.jpg';
import googleImage from './artwork/google.png';
import microsoftImage from './artwork/microsoft.png';

// Metro registers bundled images by number. Web exports supply a URL.
const source = (asset: string | number) => typeof asset === 'string' ? { uri: asset } : asset;

export const libraryArtwork = {
  depthFirst: source(depthFirstImage),
  dynamicProgramming: source(dynamicProgrammingImage),
  allTopics: source(allTopicsImage),
  contest: source(contestImage),
  contestDetail: source(contestDetailImage),
  google: source(googleImage),
  amazon: source(amazonImage),
  apple: source(appleImage),
  microsoft: source(microsoftImage),
};
