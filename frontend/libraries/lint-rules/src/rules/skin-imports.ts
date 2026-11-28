import type { Node } from 'estree';

import type { Rule, RuleContext } from '@/rules/index.js';

type Loose = { type: string; [key: string]: unknown };

/** Calls `visit` for every module a file imports or re-exports from, with the names it takes. */
function importsOf(visit: (source: string, names: string[], typeOnly: boolean, node: Node) => void) {
  const read = (node: Node) => {
    const decl = node as unknown as Loose;
    const source = decl.source as Loose | null;
    if (!source || typeof source.value !== 'string') return;
    const specifiers = ((decl.specifiers as Loose[] | undefined) ?? []).filter(
      (specifier) => (specifier.importKind ?? specifier.exportKind) !== 'type',
    );
    const names = specifiers.map((specifier) => {
      const named = (specifier.imported ?? specifier.local) as Loose | undefined;
      return typeof named?.name === 'string' ? named.name : String(named?.value ?? '*');
    });
    const typeOnly = (decl.importKind ?? decl.exportKind) === 'type' || (names.length === 0 && ((decl.specifiers as Loose[] | undefined) ?? []).length > 0);
    visit(source.value, names, typeOnly, node);
  };
  return { ImportDeclaration: read, ExportNamedDeclaration: read, ExportAllDeclaration: read };
}

const report = (context: RuleContext, node: Node, messageId: string, data: Record<string, string>) =>
  context.report({ node: node as unknown as { range: [number, number] }, messageId, data });

const SKIN_PACKAGES = /^@lib\/(orca|tamagui)-skin(\/|$)/;

export const onlyRootNamesASkin: Rule = {
  meta: {
    type: 'problem',
    docs: { description: 'A skin package is named only by the app root; every screen takes its components from @lib/skin.' },
    schema: [],
    messages: {
      onlyRootNamesASkin:
        '`{{source}}` is a skin. Only the app root (app-shell/skin/SkinRoot.tsx) names one; take components from `useSkin()` in @lib/skin, so the screen works under every skin.',
    },
  },
  create(context) {
    return importsOf((source, _names, _typeOnly, node) => {
      if (SKIN_PACKAGES.test(source)) report(context, node, 'onlyRootNamesASkin', { source });
    });
  },
};

const DESIGN_SYSTEMS = /^(@lib\/tamagui-design-system|tamagui|@tamagui\/[^/]+|lucide-react-native)(\/|$)/;

export const noDesignSystemOutsideASkin: Rule = {
  meta: {
    type: 'problem',
    docs: { description: 'A design system is imported only by the skin built on it; every screen takes its components from @lib/skin.' },
    schema: [],
    messages: {
      noDesignSystemOutsideASkin:
        '`{{source}}` is a design system. Only a skin package imports one; take the component from `useSkin()` in @lib/skin, so the other skin can draw this screen too.',
    },
  },
  create(context) {
    return importsOf((source, _names, _typeOnly, node) => {
      if (DESIGN_SYSTEMS.test(source)) report(context, node, 'noDesignSystemOutsideASkin', { source });
    });
  },
};

/** The React Native components a skin has its own entry for. */
const CONTROLS: Record<string, string> = {
  Text: 'Text',
  Button: 'Button',
  TextInput: 'TextField',
  Switch: 'Switch',
  ActivityIndicator: 'Spinner',
  Modal: 'Sheet or AlertDialog',
  TouchableOpacity: 'Button',
  TouchableHighlight: 'Button',
  TouchableWithoutFeedback: 'Button',
  TouchableNativeFeedback: 'Button',
};

export const noReactNativeControls: Rule = {
  meta: {
    type: 'problem',
    docs: { description: 'A control or text comes from the skin, not from React Native, so each skin draws it its own way.' },
    schema: [],
    messages: {
      noReactNativeControls:
        'React Native `{{name}}` looks the same under every skin. Use `{{entry}}` from `useSkin()` in @lib/skin.',
    },
  },
  create(context) {
    return importsOf((source, names, typeOnly, node) => {
      if (source !== 'react-native' || typeOnly) return;
      for (const name of names) {
        const entry = CONTROLS[name];
        if (entry) report(context, node, 'noReactNativeControls', { name, entry });
      }
    });
  },
};
