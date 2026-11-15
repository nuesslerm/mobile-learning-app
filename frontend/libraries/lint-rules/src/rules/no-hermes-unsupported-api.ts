import type { Node, Expression, Identifier, Super } from 'estree';

import type { Rule } from '@/rules/index.js';

type BannedApi = {
  /**
   * Static object identifier the call hangs off (e.g. `AbortSignal`), or
   * `null` to match the property name on any receiver (e.g. any
   * `x.throwIfAborted()`, since the AbortSignal instance is never a fixed
   * global name).
   */
  objectName: string | null;
  propertyName: string;
  message: string;
};

/**
 * JS/Web APIs confirmed NOT implemented by Hermes (React Native's JS engine)
 * despite typechecking fine and working in every browser-based check --
 * these fail only on a real device. Seeded from the AbortSignal.timeout
 * incident (see voiceEditTransport.ts's own history / docs/known-gaps.md)
 * plus independently-documented RN/Hermes issue-tracker precedent for the
 * rest of the AbortSignal family. This is a ban list of already-known gaps,
 * not a general Hermes-compatibility checker -- add an entry here the next
 * time a real one is found.
 */
const BANNED_APIS: BannedApi[] = [
  {
    objectName: 'AbortSignal',
    propertyName: 'timeout',
    message:
      'AbortSignal.timeout() is not implemented by Hermes. Use `new AbortController()` + `setTimeout(() => controller.abort(), ms)` instead -- see frontend/mla-tamagui/features/leetcode/data/voiceEditTransport.ts.',
  },
  {
    objectName: 'AbortSignal',
    propertyName: 'any',
    message:
      'AbortSignal.any() is not reliably supported in React Native (see livekit/client-sdk-react-native#4014). Listen for "abort" on each source signal and call one shared AbortController.abort() instead.',
  },
  {
    objectName: null,
    propertyName: 'getReader',
    message:
      "response.body.getReader() (fetch streaming) is not supported by React Native's fetch (spike: unverified on the shipped Hermes). Read the whole response, or use a polyfill that streams.",
  },
  {
    objectName: null,
    propertyName: 'throwIfAborted',
    message:
      'signal.throwIfAborted() is not reliably supported in React Native (see sindresorhus/ky#659). Check `signal.aborted` and throw manually instead.',
  },
];

/**
 * The Hermes feature matrix beyond the AbortSignal family. Each entry is `spike` until it has been run against the
 * shipped Hermes: the rule bans it on the documented gap, and the message says so, so a false positive is cheap to
 * lift once a device check clears it (move the entry out, with the check's evidence, into docs/known-gaps.md).
 */
type Shape =
  | { kind: 'call'; name: string }
  | { kind: 'new'; path: [string, string] }
  | { kind: 'member'; path: [string, string] };
const MATRIX: Array<{ shape: Shape; status: 'spike'; message: string }> = [
  {
    shape: { kind: 'call', name: 'structuredClone' },
    status: 'spike',
    message:
      'structuredClone() is not a Hermes global in every shipped React Native (spike: unverified on the shipped Hermes). Copy with a spread, or a small clone helper.',
  },
  {
    shape: { kind: 'new', path: ['Intl', 'Segmenter'] },
    status: 'spike',
    message:
      "Intl.Segmenter is not in Hermes' Intl subset (spike: unverified on the shipped Hermes). Split text with a regex or a library.",
  },
  {
    shape: { kind: 'member', path: ['crypto', 'subtle'] },
    status: 'spike',
    message:
      'crypto.subtle (WebCrypto) does not exist in Hermes (spike: unverified on the shipped Hermes). Use expo-crypto.',
  },
];

function dotted(node: Node): [string, string] | null {
  if (
    node.type !== 'MemberExpression' ||
    node.computed ||
    node.object.type !== 'Identifier' ||
    node.property.type !== 'Identifier'
  )
    return null;
  return [node.object.name, node.property.name];
}

function matchesBannedObject(object: Expression | Super, objectName: string): boolean {
  return object.type === 'Identifier' && (object as Identifier).name === objectName;
}

/**
 * Matches `Foo.bar(...)` / `x.throwIfAborted()`-shaped calls against
 * {@link BANNED_APIS}. Deliberately does not chase aliasing (`const {
 * timeout } = AbortSignal`, `const AS = AbortSignal`) -- these globals are
 * never re-exported/wrapped anywhere in this codebase today, and the goal is
 * catching an honest, direct call, not an adversarial rewrite.
 */
const rule: Rule = {
  meta: {
    type: 'problem',
    docs: {
      description:
        "Ban JS/Web APIs confirmed not supported by Hermes (RN's JS engine) despite typechecking fine and passing every browser-based check -- these fail only on a real device.",
    },
    schema: [],
    messages: {
      bannedApi: '{{message}}',
    },
  },
  create(context) {
    const matrix = (node: Node, wanted: (shape: Shape) => boolean) => {
      const hit = MATRIX.find((entry) => wanted(entry.shape));
      if (hit)
        context.report({
          node: node as unknown as { range: [number, number] },
          messageId: 'bannedApi',
          data: { message: hit.message },
        });
    };
    return {
      NewExpression(node: Node) {
        if (node.type !== 'NewExpression') return;
        const path = dotted(node.callee);
        if (path)
          matrix(
            node,
            (shape) =>
              shape.kind === 'new' &&
              shape.path[0] === path[0] &&
              shape.path[1] === path[1],
          );
      },
      MemberExpression(node: Node) {
        if (node.type !== 'MemberExpression') return;
        const path = dotted(node);
        if (path)
          matrix(
            node,
            (shape) =>
              shape.kind === 'member' &&
              shape.path[0] === path[0] &&
              shape.path[1] === path[1],
          );
      },
      CallExpression(node: Node) {
        if (node.type !== 'CallExpression') return;
        if (node.callee.type === 'Identifier') {
          const name = node.callee.name;
          matrix(node, (shape) => shape.kind === 'call' && shape.name === name);
        }
        const callee = node.callee;
        if (
          callee.type !== 'MemberExpression' ||
          callee.computed ||
          callee.property.type !== 'Identifier'
        ) {
          return;
        }
        const propertyName = callee.property.name;
        for (const api of BANNED_APIS) {
          if (api.propertyName !== propertyName) continue;
          if (
            api.objectName !== null &&
            !matchesBannedObject(callee.object, api.objectName)
          ) {
            continue;
          }
          context.report({
            node: node as unknown as { range: [number, number] },
            messageId: 'bannedApi',
            data: { message: api.message },
          });
          return;
        }
      },
    };
  },
};

export default rule;
