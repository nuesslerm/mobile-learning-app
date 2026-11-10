'use strict';
// @shopify/react-native-skia draws through CanvasKit, which has no runtime in
// jsdom. The Canvas stays a real element a test can address by testID; every
// drawing primitive is an inert span carrying its geometry as data attributes
// (a canvas has no DOM paths). The /lib/module/web subpath's LoadSkiaWeb
// resolves immediately and WithSkiaWeb swaps its fallback for getComponent()'s
// default export once that resolves.
const React = require('react');

const svgString = (path) =>
  typeof path === 'string'
    ? path
    : path && typeof path.toSVGString === 'function'
      ? path.toSVGString()
      : path;

// Each primitive is also drawn as SVG (path, circle, text, group), so a golden of a flow test shows what the canvas holds:
// the guide letter and any stroke the test drew.
const SVG_TAG = { Path: 'path', Circle: 'circle', Text: 'text', Group: 'g', Rect: 'rect' };
const primitive = (name) => {
  const Comp = ({ children, testID, path, color, strokeWidth, style, opacity, strokeCap, strokeJoin, ...rest }) => {
    const attrs = { 'data-skia': name };
    if (testID != null) attrs['data-testid'] = testID;
    if (path != null) attrs['data-path'] = svgString(path);
    if (color != null) attrs['data-color'] = color;
    if (strokeWidth != null) attrs['data-stroke-width'] = strokeWidth;
    for (const key of [
      'cx',
      'cy',
      'r',
      'x',
      'y',
      'width',
      'height',
      'p1',
      'p2',
      'text',
    ]) {
      if (rest[key] != null) {
        attrs[`data-${key}`] =
          typeof rest[key] === 'object' ? JSON.stringify(rest[key]) : rest[key];
      }
    }
    const tag = SVG_TAG[name] ?? 'g';
    const paint = color ?? 'black';
    if (tag === 'path') {
      attrs.d = svgString(path) ?? '';
      if (style === 'stroke') {
        Object.assign(attrs, { fill: 'none', stroke: paint, 'stroke-width': strokeWidth ?? 1 });
        if (strokeCap) attrs['stroke-linecap'] = strokeCap;
        if (strokeJoin) attrs['stroke-linejoin'] = strokeJoin;
      } else {
        attrs.fill = paint;
      }
    } else if (tag === 'circle') {
      Object.assign(attrs, { cx: rest.cx, cy: rest.cy, r: rest.r, fill: paint });
    } else if (tag === 'rect') {
      Object.assign(attrs, { x: rest.x, y: rest.y, width: rest.width, height: rest.height, fill: paint });
    } else if (tag === 'text') {
      Object.assign(attrs, { x: rest.x, y: rest.y, fill: paint });
    }
    if (opacity != null) attrs.opacity = opacity;
    return React.createElement(tag, attrs, tag === 'text' ? rest.text : children);
  };
  Comp.displayName = `Skia(${name})`;
  return Comp;
};

const Canvas = ({ children, testID, style }) =>
  React.createElement(
    'div',
    {
      'data-skia-canvas': true,
      ...(testID != null ? { 'data-testid': testID } : {}),
      style: { position: 'relative', width: '100%', height: '100%', ...(typeof style === 'object' && !Array.isArray(style) ? style : {}) },
    },
    React.createElement('svg', { width: '100%', height: '100%', style: { position: 'absolute', inset: 0, overflow: 'visible' } }, children)
  );
Canvas.displayName = 'Skia(Canvas)';

const Skia = {
  Path: {
    MakeFromSVGString: (d) => ({ toSVGString: () => d }),
  },
  Color: (c) => c,
};

const LoadSkiaWeb = () => Promise.resolve(true);

const WithSkiaWeb = ({ getComponent, fallback }) => {
  const [Component, setComponent] = React.useState(null);
  React.useEffect(() => {
    let alive = true;
    Promise.resolve()
      .then(() => getComponent())
      .then((mod) => {
        if (!alive) return;
        setComponent(() => (mod && mod.default ? mod.default : mod));
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [getComponent]);
  if (!Component) return fallback ?? null;
  return React.createElement(Component);
};
WithSkiaWeb.displayName = 'Skia(WithSkiaWeb)';

module.exports = {
  __esModule: true,
  Canvas,
  Path: primitive('Path'),
  Circle: primitive('Circle'),
  Group: primitive('Group'),
  Rect: primitive('Rect'),
  Line: primitive('Line'),
  Text: primitive('Text'),
  Skia,
  LoadSkiaWeb,
  WithSkiaWeb,
};
