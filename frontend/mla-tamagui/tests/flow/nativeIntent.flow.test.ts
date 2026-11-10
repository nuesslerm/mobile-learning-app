import { appConfig, type RoutePath } from '@lib/app-config';
import { afterEach, describe, expect, it } from 'vitest';

import { redirectSystemPath } from '../../app/+native-intent';

const baked = appConfig.initialRoute;
const bake = (route: RoutePath | undefined) => {
  appConfig.initialRoute = route;
};

describe('the native intent (wf ship eas --initial-route)', () => {
  afterEach(() => bake(baked));

  it('Given a baked route, When the app cold-starts on a bare Expo Go link, Then it opens that route', () => {
    bake('/leetcode/two-sum');

    expect(redirectSystemPath({ path: 'exp://u.expo.dev/p/group/g', initial: true })).toBe('/leetcode/two-sum');
    expect(redirectSystemPath({ path: '/', initial: true })).toBe('/leetcode/two-sum');
  });

  it('Given a baked route, When a deep link or a later link arrives, Then the link wins', () => {
    bake('/leetcode/two-sum');

    expect(redirectSystemPath({ path: '/service-report', initial: true })).toBe('/service-report');
    expect(redirectSystemPath({ path: '/', initial: false })).toBe('/');
  });

  it('Given no baked route or the root, When the app cold-starts, Then the path is untouched', () => {
    bake(undefined);
    expect(redirectSystemPath({ path: '/', initial: true })).toBe('/');

    bake('/');
    expect(redirectSystemPath({ path: 'exp://h/--/', initial: true })).toBe('exp://h/--/');
  });
});
