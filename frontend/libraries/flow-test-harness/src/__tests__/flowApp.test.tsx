import { useDependencyOverrides } from '@lib/app-di';
import { Stack, useLocalSearchParams, usePathname } from 'expo-router';
import { Text, View } from 'react-native';
import { describe, expect, it } from 'vitest';

import { createFlowApp } from '../flowApp';
import { screen } from '../index';

function RootLayout() {
  return (
    <View testID="root-layout">
      <Stack />
    </View>
  );
}

function GroupLayout() {
  return (
    <View testID="group-layout">
      <Stack />
    </View>
  );
}

function RunsLayout() {
  return (
    <View testID="runs-layout">
      <Stack />
    </View>
  );
}

function HomeScreen() {
  const { title } = useDependencyOverrides<{ title: string }>('screen');
  return (
    <View testID="home">
      <Text testID="title">{title ?? 'none'}</Text>
    </View>
  );
}

function SettingsScreen() {
  return (
    <View testID="settings">
      <Text testID="settings-path">{usePathname()}</Text>
    </View>
  );
}

function ThreadScreen() {
  const { threadId } = useLocalSearchParams<{ threadId: string }>();
  return (
    <View testID="thread">
      <Text testID="thread-id">{threadId}</Text>
      <Text testID="thread-path">{usePathname()}</Text>
    </View>
  );
}

function RunsIndexScreen() {
  return (
    <View testID="runs-index">
      <Text testID="runs-index-path">{usePathname()}</Text>
    </View>
  );
}

function RunScreen() {
  const { runId, tab } = useLocalSearchParams<{
    runId: string;
    tab?: string;
  }>();
  return (
    <View testID="run">
      <Text testID="run-id">{runId}</Text>
      <Text testID="run-tab">{tab ?? 'none'}</Text>
    </View>
  );
}

function RunSummaryScreen() {
  return <View testID="run-summary" />;
}

function useScreenTitle() {
  return useDependencyOverrides<{ title: string }>('screen').title;
}

const routes = {
  '../routes/_layout.tsx': { default: RootLayout },
  '../routes/(group)/_layout.tsx': { default: GroupLayout },
  '../routes/(group)/index.tsx': { default: HomeScreen },
  '../routes/settings.tsx': { default: SettingsScreen },
  '../routes/[threadId].tsx': { default: ThreadScreen },
  '../routes/runs/_layout.tsx': { default: RunsLayout },
  '../routes/runs/index.tsx': { default: RunsIndexScreen },
  '../routes/runs/[runId].tsx': { default: RunScreen },
  '../routes/runs/summary.tsx': { default: RunSummaryScreen },
};

const app = createFlowApp({
  routes,
  routesRoot: '../routes/',
  basePath: '/coding-agent',
});
const withRootLayout = createFlowApp({
  routes,
  routesRoot: '../routes/',
  basePath: '/coding-agent',
  includeRootLayout: true,
});
const lazyApp = createFlowApp({
  routes: {
    '../routes/index.tsx': () => Promise.resolve({ default: HomeScreen }),
    '../routes/broken.tsx': () => Promise.reject(new Error('import failed')),
  },
  routesRoot: '../routes/',
  basePath: '/lazy',
});

describe('createFlowApp', () => {
  it('given a (group) route, when rendered at the group root, then the group layout wraps the group screen and the root layout is skipped', async () => {
    await app.render('/coding-agent');
    expect(screen.getByTestId('home')).toBeTruthy();
    expect(screen.getByTestId('group-layout')).toBeTruthy();
    expect(screen.queryByTestId('root-layout')).toBeNull();
  });

  it('given an index route, when rendered at its directory path, then the index screen renders', async () => {
    await app.render('/coding-agent/runs');
    expect(screen.getByTestId('runs-index')).toBeTruthy();
    expect(screen.getByTestId('runs-index-path').textContent).toBe(
      '/coding-agent/runs'
    );
  });

  it('given a [param] route, when rendered at a matching path, then the segment becomes a param and the pathname is seeded', async () => {
    await app.render('/coding-agent/thread-1');
    expect(screen.getByTestId('thread-id').textContent).toBe('thread-1');
    expect(screen.getByTestId('thread-path').textContent).toBe(
      '/coding-agent/thread-1'
    );
  });

  it('given static and dynamic routes in one directory, when rendered at the static path, then the static screen wins', async () => {
    await app.render('/coding-agent/runs/summary');
    expect(screen.getByTestId('run-summary')).toBeTruthy();
    expect(screen.queryByTestId('run')).toBeNull();
  });

  it('given a dynamic route in one directory, when rendered at a non-static path, then the dynamic screen captures the param', async () => {
    await app.render('/coding-agent/runs/run-1');
    expect(screen.getByTestId('run-id').textContent).toBe('run-1');
  });

  it('given nested layouts, when includeRootLayout is true, then the chain renders root outside group outside screen', async () => {
    await withRootLayout.render('/coding-agent');
    const root = screen.getByTestId('root-layout');
    const group = screen.getByTestId('group-layout');
    expect(root.contains(group)).toBe(true);
    expect(group.contains(screen.getByTestId('home'))).toBe(true);
  });

  it('given the runs layout, when includeRootLayout is true, then the chain is root, runs, screen', async () => {
    await withRootLayout.render('/coding-agent/runs/run-1');
    const root = screen.getByTestId('root-layout');
    const runs = screen.getByTestId('runs-layout');
    expect(root.contains(runs)).toBe(true);
    expect(runs.contains(screen.getByTestId('run'))).toBe(true);
  });

  it('given a search record, when rendering a param route, then search entries are merged into params', async () => {
    await app.render('/coding-agent/runs/run-1', { search: { tab: 'logs' } });
    expect(screen.getByTestId('run-id').textContent).toBe('run-1');
    expect(screen.getByTestId('run-tab').textContent).toBe('logs');
  });

  it('given overrides, when rendering a screen that reads useDependencyOverrides, then the override value renders', async () => {
    await app.render('/coding-agent', {
      overrides: { screen: { title: 'Hi' } },
    });
    expect(screen.getByTestId('title').textContent).toBe('Hi');
  });

  it('given no overrides, when rendering, then useDependencyOverrides falls back to the empty object', async () => {
    await app.render('/coding-agent');
    expect(screen.getByTestId('title').textContent).toBe('none');
  });

  it('given lazy-loader routes, when an unmatched loader rejects, then the matched route still renders', async () => {
    await lazyApp.render('/lazy');
    expect(screen.getByTestId('home')).toBeTruthy();
  });

  it('given renderHook with overrides, when the hook reads useDependencyOverrides, then the override value is returned', () => {
    const { result } = app.renderHook(useScreenTitle, {
      overrides: { screen: { title: 'HookHi' } },
    });
    expect(result.current).toBe('HookHi');
  });

  it('given renderHook without overrides, when the hook reads useDependencyOverrides, then it returns undefined', () => {
    const { result } = app.renderHook(useScreenTitle);
    expect(result.current).toBeUndefined();
  });

  it('given a path with no matching route, when rendered, then an error names the path', async () => {
    await expect(app.render('/coding-agent/nope/nope')).rejects.toThrow(
      'createFlowApp: no route for /coding-agent/nope/nope'
    );
  });
});
