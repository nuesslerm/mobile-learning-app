import { createDeferredEngine, LANGUAGE_NAMES } from '../src';
import { createFakeEditor, editorContract, semanticContract, syntaxTreeContract, type FakeEditor, type MakeSubject } from '../src/testing';
import { lezerSnapshot } from './support/lezerPlain';

const errors: unknown[] = [];

const plain: MakeSubject = async ({ bound }) => {
  const deferred = createDeferredEngine((error) => errors.push(error));
  let generation = 0;
  let current: FakeEditor | null = null;
  const bind = () => {
    generation += 1;
    current = createFakeEditor({ instance: `fake-${generation}`, text: '' });
    deferred.bind(current.engine);
  };
  if (bound) bind();
  return {
    engine: deferred,
    dispose: () => undefined,
    hooks: {
      userTypes: (text) => current?.userTypes(text),
      userSelects: (anchor, head) => current?.userSelects(anchor, head),
      restart: bind,
    },
  };
};

editorContract('the fake editor, directly', plain, { languages: LANGUAGE_NAMES });

const withTree: MakeSubject = async ({ bound }) => {
  const deferred = createDeferredEngine((error) => errors.push(error));
  let generation = 0;
  let current: FakeEditor | null = null;
  const bind = () => {
    generation += 1;
    current = createFakeEditor({ instance: `fake-${generation}`, text: '', tree: true });
    deferred.bind(current.engine);
  };
  if (bound) bind();
  return { engine: deferred, dispose: () => undefined, hooks: { restart: bind, slowParse: (calls) => current?.slowParse?.(calls) } };
};

// the same editor over trees from the installed grammars, as the page answers them
const withLezerTree: MakeSubject = async ({ bound }) => {
  const deferred = createDeferredEngine((error) => errors.push(error));
  if (bound) deferred.bind(createFakeEditor({ instance: 'fake-lezer', text: '', tree: lezerSnapshot }).engine);
  return { engine: deferred, dispose: () => undefined, hooks: {} };
};

for (const language of LANGUAGE_NAMES) syntaxTreeContract('the fake editor, directly', withTree, language);
for (const language of LANGUAGE_NAMES) semanticContract('the fake editor, directly', plain, language);
for (const language of LANGUAGE_NAMES) semanticContract('the fake editor with its own parsers, directly', withTree, language);
for (const language of LANGUAGE_NAMES) semanticContract('the fake editor on Lezer trees, directly', withLezerTree, language);
