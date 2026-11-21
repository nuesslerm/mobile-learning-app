import { createDeferredEngine, LANGUAGE_NAMES } from '../src';
import { createFakeEditor, editorContract, type FakeEditor } from '../src/testing';

const errors: unknown[] = [];

editorContract('the fake editor, directly', async ({ bound }) => {
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
}, { languages: LANGUAGE_NAMES });
