'use strict';
// Callable module object: `import Markdown from 'react-native-markdown-display'`
// may yield the whole module under CommonJS interop, so the module itself is
// the component. Renders the markdown source as plain text.
const React = require('react');
const RN = require('react-native-web');
const Markdown = (props) => React.createElement(RN.Text, { testID: props.testID }, props.children);
Markdown.displayName = 'MarkdownStub';
const fixed = {
  __esModule: true,
  MarkdownIt: function MarkdownIt() {
    return { use() { return this; }, render: (s) => String(s) };
  },
  renderRules: {},
  styles: {},
};
module.exports = Object.assign(Markdown, fixed, { default: Markdown });
