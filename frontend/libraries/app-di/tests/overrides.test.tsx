import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import {
  DependencyOverridesProvider,
  useDependencyOverrides,
} from '../src/index';

function ReadOverride({ name }: { name: string }) {
  return <span data-key={name}>{JSON.stringify(useDependencyOverrides(name))}</span>;
}

function ReadOverrides({ keys }: { keys: string[] }) {
  return (
    <div>
      {keys.map((key) => (
        <ReadOverride key={key} name={key} />
      ))}
    </div>
  );
}

describe('useDependencyOverrides', () => {
  it('returns {} without a provider', () => {
    // Given a component that reads overrides with no provider in scope
    // When rendered to static markup
    const html = renderToStaticMarkup(
      <ReadOverrides keys={['alpha', 'beta']} />
    );
    // Then every read falls back to the empty object
    expect(html).toContain('data-key="alpha">{}</span>');
    expect(html).toContain('data-key="beta">{}</span>');
  });

  it('keeps keys independent under one provider', () => {
    // Given a provider with two unrelated override keys
    // When rendered
    const html = renderToStaticMarkup(
      <DependencyOverridesProvider
        overrides={{ alpha: { a: 1 }, beta: { b: 2 } }}
      >
        <ReadOverrides keys={['alpha', 'beta']} />
      </DependencyOverridesProvider>
    );
    // Then each key returns its own value without leaking across keys
    expect(html).toContain('data-key="alpha">{&quot;a&quot;:1}</span>');
    expect(html).toContain('data-key="beta">{&quot;b&quot;:2}</span>');
  });

  it('nested providers: inner wins per key', () => {
    // Given nested providers where the inner overrides one key and leaves another untouched
    // When rendered
    const html = renderToStaticMarkup(
      <DependencyOverridesProvider
        overrides={{ theme: { mode: 'dark' }, user: { id: 1 } }}
      >
        <DependencyOverridesProvider overrides={{ theme: { mode: 'light' } }}>
          <ReadOverrides keys={['theme', 'user']} />
        </DependencyOverridesProvider>
      </DependencyOverridesProvider>
    );
    // Then the inner provider wins for the overridden key
    expect(html).toContain(
      'data-key="theme">{&quot;mode&quot;:&quot;light&quot;}</span>'
    );
    // And the outer provider still supplies the untouched key
    expect(html).toContain('data-key="user">{&quot;id&quot;:1}</span>');
  });
});
