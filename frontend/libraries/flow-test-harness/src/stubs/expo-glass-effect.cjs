'use strict';
// The real package is plain JavaScript off iOS (a View, and false for both checks); setLiquidGlass in the harness turns the native branch on.
const React = require('react');
const RN = require('react-native-web');
const g = globalThis;
const available = () => g.__flowGlass?.available === true;
const styleOf = (value) => (typeof value === 'object' && value !== null ? value.style : value) ?? 'regular';

const GlassView = ({ glassEffectStyle, tintColor, isInteractive, colorScheme, children, ...rest }) =>
  React.createElement(
    RN.View,
    {
      ...rest,
      dataSet: available()
        ? { glassStyle: styleOf(glassEffectStyle), glassTint: tintColor === undefined ? undefined : String(tintColor), glassInteractive: String(isInteractive === true), glassColorScheme: colorScheme ?? 'auto' }
        : undefined,
    },
    children
  );
const GlassContainer = ({ spacing, children, ...rest }) =>
  React.createElement(RN.View, { ...rest, dataSet: available() ? { glassContainer: 'true', glassSpacing: String(spacing ?? 0) } : undefined }, children);

module.exports = { __esModule: true, GlassView, GlassContainer, isLiquidGlassAvailable: available, isGlassEffectAPIAvailable: available };
