import { appConfig } from '@lib/app-config';
import { captureRunId } from '../app-shell/runId';

// A link that names no screen: `/`, a URL without Expo Go's `/--/` mount (an EAS Update link), or the mount alone.
const isBareLaunch = (path: string): boolean => {
  const mount = path.indexOf('/--/');
  const route = mount >= 0 ? path.slice(mount + 3) : /^[a-z][a-z0-9+.-]*:\/\//i.test(path) ? '' : path;
  const bare = route.split(/[?#]/)[0];
  return bare === '' || bare === '/';
};

// `wf ship eas --initial-route <route>` bakes the route into appConfig; a cold start without a deep link opens it.
export function redirectSystemPath({ path, initial }: { path: string; initial: boolean }): string {
  if (initial) captureRunId(path);
  const route = appConfig.initialRoute;
  return initial && route && route !== '/' && isBareLaunch(path) ? route : path;
}
