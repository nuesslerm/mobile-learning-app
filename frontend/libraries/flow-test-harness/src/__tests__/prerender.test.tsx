import { useLocalSearchParams, usePathname } from 'expo-router';
import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { Text } from 'react-native';
import { describe, expect, it } from 'vitest';

import { createFlowApp } from '../flowApp';
import { hydrationErrors, waitFor } from '../index';

const order: string[] = [];

function Hydrating() {
  const [phase, setPhase] = useState('server');
  useEffect(() => {
    order.push('effect');
    setPhase('hydrated');
  }, []);
  return <Text testID="phase">{phase}</Text>;
}

function WithFrame() {
  useEffect(() => {
    order.push('effect');
  }, []);
  return (
    <iframe
      title="page"
      srcDoc="<body><script>window.parent.postMessage('ready', '*')</script></body>"
    />
  );
}

function WithPortal() {
  const [mounted, setMounted] = useState(false);
  const [taps, setTaps] = useState(0);
  useEffect(() => setMounted(true), []);
  return (
    <>
      <Text testID="taps">{String(taps)}</Text>
      {mounted &&
        createPortal(
          <button type="button" data-testid="portaled" onClick={() => setTaps((n) => n + 1)}>
            key
          </button>,
          document.body
        )}
    </>
  );
}

function Lesson() {
  const { lessonId } = useLocalSearchParams<{ lessonId: string }>();
  return (
    <Text testID="lesson">
      {lessonId} at {usePathname()}
    </Text>
  );
}

function Plain() {
  return <Text testID="plain">plain</Text>;
}

const app = createFlowApp({
  routes: {
    '../routes/phase.tsx': { default: Hydrating },
    '../routes/frame.tsx': { default: WithFrame },
    '../routes/portal.tsx': { default: WithPortal },
    '../routes/plain.tsx': { default: Plain },
    '../routes/lessons/[lessonId].tsx': { default: Lesson },
    '../routes/listed/[lessonId].tsx': {
      default: Lesson,
      generateStaticParams: () => [{ lessonId: 'one' }, { lessonId: 'two' }],
    },
  },
  routesRoot: '../routes/',
});

describe('given app.render with prerender', () => {
  it('when a screen sets state in an effect, then the server HTML shows first and hydration takes over', async () => {
    const session = await app.render('/phase', { prerender: true });
    await waitFor(() => expect(session.getByTestId('phase').textContent).toBe('hydrated'));
  });

  it('when the prerendered tree holds an iframe, then the page has loaded and posted before any effect runs', async () => {
    order.length = 0;
    const onMessage = (event: MessageEvent) => {
      if (event.data === 'ready') order.push('ready');
    };
    window.addEventListener('message', onMessage);
    await app.render('/frame', { prerender: true });
    await waitFor(() => expect(order).toContain('effect'));
    window.removeEventListener('message', onMessage);
    expect(order.indexOf('ready')).toBeGreaterThanOrEqual(0);
    expect(order.indexOf('ready')).toBeLessThan(order.indexOf('effect'));
  });

  it('when the screen portals a control to the body after hydration, then the session taps it', async () => {
    const session = await app.render('/portal', { prerender: true });
    await waitFor(() => expect(session.getByTestId('portaled')).toBeTruthy());

    await session.tap('portaled');

    await waitFor(() => expect(session.getByTestId('taps').textContent).toBe('1'));
  });

  it('when the path is a dynamic route with no static params, then the server HTML is its template page and hydration warns', async () => {
    // Given a route the export holds only as its template page (what Expo writes as lessons/[lessonId].html)
    // and a browser that hydrates it at /lessons/one
    const caught = hydrationErrors();
    // When it is rendered as the export serves a cold deep link
    const session = await app.render('/lessons/one', { prerender: 'export' });
    // Then React reports that the server's text (the template's [lessonId]) differs from the browser's, and the page recovers
    await waitFor(() => expect(caught.seen.join('\n')).toContain('Hydration failed because the server rendered text'));
    caught.stop();
    expect(session.getByTestId('lesson').textContent).toBe('one at /lessons/one');
  });

  it('when the dynamic route lists the path in generateStaticParams, then the server renders that page and hydration is clean', async () => {
    // Given a route whose export has a page per listed id
    const caught = hydrationErrors();
    // When /listed/two is opened cold
    const session = await app.render('/listed/two', { prerender: 'export' });
    caught.stop();
    // Then the hydrated page shows the id and React reported nothing
    expect(session.getByTestId('lesson').textContent).toBe('two at /listed/two');
    expect(caught.seen).toEqual([]);
  });

  it('when the dynamic route lists other ids than the path, then the path falls back to the template page and hydration warns', async () => {
    // Given the same route, and an id its export has no page for
    const caught = hydrationErrors();
    // When it is opened cold
    const session = await app.render('/listed/three', { prerender: 'export' });
    // Then it hydrates from the template page, which differs from what the browser renders
    await waitFor(() => expect(caught.seen.join('\n')).toContain('Hydration failed because the server rendered text'));
    caught.stop();
    expect(session.getByTestId('lesson').textContent).toBe('three at /listed/three');
  });

  it('when the path is a static route, then the export page is the route itself and hydration is clean', async () => {
    const caught = hydrationErrors();
    const session = await app.render('/plain', { prerender: 'export' });
    caught.stop();
    expect(session.getByTestId('plain').textContent).toBe('plain');
    expect(caught.seen).toEqual([]);
  });
});
