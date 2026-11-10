import type { TextStyle } from 'react-native';

const CODE_PALETTE_IDS = [
  'one-dark',
  'monokai',
  'solarized-dark',
  'github-light',
] as const;

export type CodePaletteId = (typeof CODE_PALETTE_IDS)[number];

export type CodePalette = {
  id: CodePaletteId;
  name: string;
  appearance: 'light' | 'dark';
  background: string;
  defaultText: string;
  cursor: string;
  selection: string;
  classes: Record<
    string,
    { color: string; fontStyle?: TextStyle['fontStyle'] }
  >;
};

export const DEFAULT_PALETTE_ID: CodePaletteId = 'one-dark';

const ONE_DARK: CodePalette = {
  id: 'one-dark',
  name: 'One Dark',
  appearance: 'dark',
  background: '#282c34',
  defaultText: '#abb2bf',
  cursor: '#528bff',
  selection: 'rgba(82, 139, 255, 0.3)',
  classes: {
    'hljs-keyword': { color: '#c678dd' },
    'hljs-built_in': { color: '#56b6c2' },
    'hljs-type': { color: '#e5c07b' },
    'hljs-literal': { color: '#d19a66' },
    'hljs-number': { color: '#d19a66' },
    'hljs-string': { color: '#98c379' },
    'hljs-comment': { color: '#7f848e', fontStyle: 'italic' },
    'hljs-doctag': { color: '#7f848e' },
    'hljs-function': { color: '#61afef' },
    'hljs-title': { color: '#61afef' },
    'hljs-title.function_': { color: '#61afef' },
    'hljs-title.class_': { color: '#e5c07b' },
    'hljs-params': { color: '#abb2bf' },
    'hljs-variable': { color: '#e06c75' },
    'hljs-attr': { color: '#d19a66' },
    'hljs-meta': { color: '#e5c07b' },
    'hljs-selector-tag': { color: '#e06c75' },
    'hljs-selector-class': { color: '#e5c07b' },
    'hljs-punctuation': { color: '#abb2bf' },
    'hljs-operator': { color: '#56b6c2' },
    'hljs-property': { color: '#e06c75' },
    'hljs-preprocessor': { color: '#e5c07b' },
  },
};

const MONOKAI: CodePalette = {
  id: 'monokai',
  name: 'Monokai',
  appearance: 'dark',
  background: '#272822',
  defaultText: '#f8f8f2',
  cursor: '#f92672',
  selection: 'rgba(249, 38, 114, 0.25)',
  classes: {
    'hljs-keyword': { color: '#f92672' },
    'hljs-built_in': { color: '#66d9ef' },
    'hljs-type': { color: '#66d9ef', fontStyle: 'italic' },
    'hljs-literal': { color: '#ae81ff' },
    'hljs-number': { color: '#ae81ff' },
    'hljs-string': { color: '#e6db74' },
    'hljs-comment': { color: '#75715e', fontStyle: 'italic' },
    'hljs-doctag': { color: '#75715e' },
    'hljs-function': { color: '#a6e22e' },
    'hljs-title': { color: '#a6e22e' },
    'hljs-title.function_': { color: '#a6e22e' },
    'hljs-title.class_': { color: '#a6e22e' },
    'hljs-params': { color: '#f8f8f2' },
    'hljs-variable': { color: '#f8f8f2' },
    'hljs-attr': { color: '#a6e22e' },
    'hljs-meta': { color: '#f8f8f2' },
    'hljs-selector-tag': { color: '#f92672' },
    'hljs-selector-class': { color: '#a6e22e' },
    'hljs-punctuation': { color: '#f8f8f2' },
    'hljs-operator': { color: '#f92672' },
    'hljs-property': { color: '#66d9ef' },
    'hljs-preprocessor': { color: '#f92672' },
  },
};

const SOLARIZED_DARK: CodePalette = {
  id: 'solarized-dark',
  name: 'Solarized',
  appearance: 'dark',
  background: '#002b36',
  defaultText: '#839496',
  cursor: '#268bd2',
  selection: 'rgba(38, 139, 210, 0.25)',
  classes: {
    'hljs-keyword': { color: '#859900' },
    'hljs-built_in': { color: '#2aa198' },
    'hljs-type': { color: '#b58900' },
    'hljs-literal': { color: '#cb4b16' },
    'hljs-number': { color: '#d33682' },
    'hljs-string': { color: '#2aa198' },
    'hljs-comment': { color: '#586e75', fontStyle: 'italic' },
    'hljs-doctag': { color: '#586e75' },
    'hljs-function': { color: '#268bd2' },
    'hljs-title': { color: '#268bd2' },
    'hljs-title.function_': { color: '#268bd2' },
    'hljs-title.class_': { color: '#b58900' },
    'hljs-params': { color: '#839496' },
    'hljs-variable': { color: '#cb4b16' },
    'hljs-attr': { color: '#b58900' },
    'hljs-meta': { color: '#cb4b16' },
    'hljs-selector-tag': { color: '#859900' },
    'hljs-selector-class': { color: '#268bd2' },
    'hljs-punctuation': { color: '#839496' },
    'hljs-operator': { color: '#859900' },
    'hljs-property': { color: '#268bd2' },
    'hljs-preprocessor': { color: '#cb4b16' },
  },
};

const GITHUB_LIGHT: CodePalette = {
  id: 'github-light',
  name: 'GitHub',
  appearance: 'light',
  background: '#ffffff',
  defaultText: '#24292e',
  cursor: '#0366d6',
  selection: 'rgba(3, 102, 214, 0.2)',
  classes: {
    'hljs-keyword': { color: '#d73a49' },
    'hljs-built_in': { color: '#005cc5' },
    'hljs-type': { color: '#e36209' },
    'hljs-literal': { color: '#005cc5' },
    'hljs-number': { color: '#005cc5' },
    'hljs-string': { color: '#032f62' },
    'hljs-comment': { color: '#6a737d', fontStyle: 'italic' },
    'hljs-doctag': { color: '#6a737d' },
    'hljs-function': { color: '#6f42c1' },
    'hljs-title': { color: '#6f42c1' },
    'hljs-title.function_': { color: '#6f42c1' },
    'hljs-title.class_': { color: '#e36209' },
    'hljs-params': { color: '#24292e' },
    'hljs-variable': { color: '#e36209' },
    'hljs-attr': { color: '#005cc5' },
    'hljs-meta': { color: '#e36209' },
    'hljs-selector-tag': { color: '#22863a' },
    'hljs-selector-class': { color: '#6f42c1' },
    'hljs-punctuation': { color: '#24292e' },
    'hljs-operator': { color: '#d73a49' },
    'hljs-property': { color: '#005cc5' },
    'hljs-preprocessor': { color: '#d73a49' },
  },
};

export const CODE_PALETTES: Record<CodePaletteId, CodePalette> = {
  'one-dark': ONE_DARK,
  monokai: MONOKAI,
  'solarized-dark': SOLARIZED_DARK,
  'github-light': GITHUB_LIGHT,
};
