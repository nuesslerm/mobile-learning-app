import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import {
  DependencyOverridesProvider,
  overrideKey,
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

describe('overrideKey', () => {
  type Ports = { clock: () => number };
  const portsKey = overrideKey<Ports>('ports');

  function ReadTyped() {
    const ports = useDependencyOverrides(portsKey);
    return <span>{ports.clock ? String(ports.clock()) : 'none'}</span>;
  }

  it('reads the override a provider gave under the same name, typed', () => {
    // Given a provider that overrides the "ports" key with a clock
    // When a component reads it through the typed key
    const html = renderToStaticMarkup(
      <DependencyOverridesProvider overrides={{ ports: { clock: () => 7 } }}>
        <ReadTyped />
      </DependencyOverridesProvider>
    );
    // Then it gets that clock, and the string form of the same name reads the same object
    expect(html).toContain('7');
    expect(portsKey.name).toBe('ports');
  });

  it('falls back to an empty object without a provider', () => {
    // Given no provider
    // When the typed key is read
    const html = renderToStaticMarkup(<ReadTyped />);
    // Then nothing is overridden
    expect(html).toContain('none');
  });
});
