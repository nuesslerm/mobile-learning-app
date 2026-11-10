// @vitest-environment jsdom
import type { ReactElement } from 'react';
import { act } from 'react';
import type { Root } from 'react-dom/client';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { TamaguiProvider } from '../../primitives';
import { tamaguiConfig } from '../../theme/tamagui.config';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../Tabs';

// jsdom lacks these and parts of Tamagui's web bundle touch them at import
// time (e.g. @tamagui/select's trigger reads window.matchMedia), so they
// must exist before the `tamagui` import above is evaluated.
vi.hoisted(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  const w = globalThis.window as unknown as Record<string, unknown>;
  if (typeof w.matchMedia !== 'function') {
    w.matchMedia = (query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList;
  }
  if (typeof w.ResizeObserver !== 'function') {
    class ResizeObserverStub {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    w.ResizeObserver = ResizeObserverStub;
  }
});

const renderedRoots: Root[] = [];

function render(element: ReactElement) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  renderedRoots.push(root);
  act(() => {
    root.render(
      <TamaguiProvider config={tamaguiConfig} defaultTheme="light">
        {element}
      </TamaguiProvider>
    );
  });
  return container;
}

afterEach(() => {
  for (const root of renderedRoots.splice(0)) {
    act(() => root.unmount());
  }
  document.body.innerHTML = '';
});

function triggers(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll('[role="tab"]'));
}

function triggerByLabel(
  container: HTMLElement,
  label: string
): HTMLElement | undefined {
  return triggers(container).find((el) => el.textContent === label);
}

function pressTab(container: HTMLElement, label: string): void {
  const trigger = triggerByLabel(container, label);
  if (!trigger) throw new Error(`no tab trigger with label "${label}"`);
  act(() => {
    trigger.dispatchEvent(
      new MouseEvent('click', { bubbles: true, cancelable: true })
    );
  });
}

const tabsFixture = (onValueChange?: (value: string) => void) => (
  <Tabs defaultValue="swipe" onValueChange={onValueChange}>
    <TabsList>
      <TabsTrigger value="swipe">Swipe</TabsTrigger>
      <TabsTrigger value="overlay">Overlay</TabsTrigger>
      <TabsTrigger value="side-by-side">Side by side</TabsTrigger>
    </TabsList>
    <TabsContent value="swipe">
      <span>swipe-content</span>
    </TabsContent>
    <TabsContent value="overlay">
      <span>overlay-content</span>
    </TabsContent>
    <TabsContent value="side-by-side">
      <span>side-by-side-content</span>
    </TabsContent>
  </Tabs>
);

describe('given Tabs with a defaultValue', () => {
  it('when rendered, then only the default tab content is mounted and its trigger is active', () => {
    const container = render(tabsFixture());

    const panel = container.querySelector('[role="tabpanel"]');
    expect(panel).not.toBeNull();
    expect(panel?.textContent).toContain('swipe-content');
    expect(panel?.textContent).not.toContain('overlay-content');
    expect(container.textContent).not.toContain('overlay-content');
    expect(container.textContent).not.toContain('side-by-side-content');

    const active = triggerByLabel(container, 'Swipe');
    expect(active?.getAttribute('aria-selected')).toBe('true');
    expect(active?.getAttribute('data-state')).toBe('active');
    expect(
      triggerByLabel(container, 'Overlay')?.getAttribute('data-state')
    ).toBe('inactive');
  });
});

describe('given a rendered Tabs with a default tab selected', () => {
  it('when another trigger is pressed, then onValueChange fires with the new value and its content replaces the old one', () => {
    const onValueChange = vi.fn();
    const container = render(tabsFixture(onValueChange));

    pressTab(container, 'Overlay');

    expect(onValueChange).toHaveBeenCalledWith('overlay');

    const panel = container.querySelector('[role="tabpanel"]');
    expect(panel?.textContent).toContain('overlay-content');
    expect(container.textContent).not.toContain('swipe-content');

    expect(
      triggerByLabel(container, 'Overlay')?.getAttribute('data-state')
    ).toBe('active');
    expect(triggerByLabel(container, 'Swipe')?.getAttribute('data-state')).toBe(
      'inactive'
    );
  });
});
