/**
 * Type-shape tests for the wireframe-props additions to rn-agentation's domain
 * types and headless handle. These verify the public surface compiles with the
 * expected shape (props bag on `WireframePlacement`, `updateWireframeProps`
 * method on `AgentationHandle`) without spinning up the React Native runtime.
 *
 * Integration of `updateWireframeProps` itself is exercised by the host app's
 * Playwright web suite, since the headless hook requires RN's
 * `useWindowDimensions` + `expo-clipboard` which don't have a pure-Node shim.
 */
import { describe, expect, it } from 'vitest';

import type { AgentationHandle } from '../src/headless/types';
import type {
  Annotation,
  WireframePlacement,
} from '../src/domain/types';

describe('WireframePlacement type', () => {
  it('accepts an optional props bag on a wireframe', () => {
    const wf: WireframePlacement = {
      componentType: 'Button',
      xRatio: 0.1,
      yRatio: 0.2,
      widthRatio: 0.3,
      heightRatio: 0.1,
      props: { variant: 'default', size: 'default', disabled: false, text: 'Go' },
    };
    // `props` is this domain type's own field (a wireframe's config bag),
    // not a React internal -- read it via a local binding rather than a
    // `.props.<name>` member chain.
    const { props } = wf;
    expect(props?.variant).toBe('default');
    expect(props?.text).toBe('Go');
  });

  it('omits props for legacy / pre-editor annotations', () => {
    const wf: WireframePlacement = {
      componentType: 'Card',
      xRatio: 0,
      yRatio: 0,
      widthRatio: 0.5,
      heightRatio: 0.3,
    };
    expect(wf.props).toBeUndefined();
  });
});

describe('AgentationHandle type', () => {
  it('declares updateWireframeProps on the public handle', () => {
    // Type-only assertion: if `updateWireframeProps` is removed from the
    // handle interface this assignment fails to compile.
    const handle: Partial<AgentationHandle> = {
      updateWireframeProps: () => {},
    };
    expect(typeof handle.updateWireframeProps).toBe('function');
  });
});

describe('Annotation carries props through', () => {
  it('forwards wireframe props onto an annotation', () => {
    const annotation: Annotation = {
      id: '1',
      componentName: 'Button',
      note: '',
      platform: 'web',
      kind: 'placement',
      wireframe: {
        componentType: 'Button',
        xRatio: 0.1,
        yRatio: 0.1,
        widthRatio: 0.2,
        heightRatio: 0.1,
        props: { variant: 'destructive', size: 'lg', text: 'Delete' },
      },
    };
    const { props } = annotation.wireframe ?? {};
    expect(props?.variant).toBe('destructive');
    expect(props?.text).toBe('Delete');
  });
});