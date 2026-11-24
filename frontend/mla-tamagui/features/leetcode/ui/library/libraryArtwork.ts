import resumeImage from './artwork/resume.png';
import profileImage from './artwork/profile.png';
import allTopicsImage from './artwork/all-topics.png';
import dynamicProgrammingImage from './artwork/dynamic-programming.png';
import depthFirstImage from './artwork/depth-first.png';
import contestDetailImage from './artwork/contest-detail.jpg';
import contestImage from './artwork/contest.jpg';

// Metro registers bundled images by number. Web exports supply a URL.
const source = (asset: string | number) => typeof asset === 'string' ? { uri: asset } : asset;

export const libraryArtwork = {
  depthFirst: source(depthFirstImage),
  dynamicProgramming: source(dynamicProgrammingImage),
  allTopics: source(allTopicsImage),
  profile: source(profileImage),
  resume: source(resumeImage),
  contest: source(contestImage),
  contestDetail: source(contestDetailImage),
};
