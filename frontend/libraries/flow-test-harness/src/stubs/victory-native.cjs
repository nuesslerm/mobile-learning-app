'use strict';
// victory-native draws through @shopify/react-native-skia, which has no runtime in jsdom. The chart's own logic
// (targets, press wiring, empty state) lives outside the canvas, so the canvas marks are inert; pixels are the visual judge's job.
const React = require('react');
const mark = (name) => {
  const Mark = () => React.createElement('span', { 'data-chart-mark': name });
  Mark.displayName = `Victory(${name})`;
  return Mark;
};
const CartesianChart = ({ children, data }) =>
  React.createElement(
    'div',
    { 'data-chart': 'CartesianChart', 'data-points': Array.isArray(data) ? data.length : 0 },
    typeof children === 'function'
      ? children({ points: {}, chartBounds: { left: 0, right: 300, top: 0, bottom: 200 }, canvasSize: { width: 300, height: 200 } })
      : children
  );
const PolarChart = ({ children }) => React.createElement('div', { 'data-chart': 'PolarChart' }, children);
module.exports = {
  __esModule: true,
  CartesianChart,
  PolarChart,
  Bar: mark('Bar'),
  BarGroup: mark('BarGroup'),
  StackedBar: mark('StackedBar'),
  StackedArea: mark('StackedArea'),
  Line: mark('Line'),
  Area: mark('Area'),
  Scatter: mark('Scatter'),
  Pie: { Chart: mark('Pie.Chart'), Slice: mark('Pie.Slice') },
  useChartPressState: () => ({ state: { x: { value: { value: 0 } }, y: {} }, isActive: false }),
};
