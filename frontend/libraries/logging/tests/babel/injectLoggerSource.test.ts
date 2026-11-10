import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { transform } from '@babel/core';
import { describe, it, expect } from 'vitest';

import injectLoggerSource from '../../src/babel/injectLoggerSource';

const FIXTURES_DIR = fileURLToPath(new URL('./fixtures/', import.meta.url));

function transformFixture(
  fixtureName: string,
  filenameOverride?: string
): string {
  const code = readFileSync(`${FIXTURES_DIR}${fixtureName}`, 'utf8');
  const result = transform(code, {
    filename: filenameOverride ?? `tests/babel/fixtures/${fixtureName}`,
    configFile: false,
    babelrc: false,
    presets: [['@babel/preset-typescript', { allExtensions: true }]],
    plugins: [injectLoggerSource],
    cwd: process.cwd(),
  });
  if (!result?.code)
    throw new Error(`transform produced no output for ${fixtureName}`);
  return result.code;
}

describe('injectLoggerSource babel plugin', () => {
  describe('given Logger.info(...) is called inside a named function', () => {
    it('when transformed, then it injects file/line/fn/fnPath via Logger.withContext({...})', () => {
      const out = transformFixture('basicFunctionCall.ts');

      expect(out).toContain('Logger.withContext({');
      expect(out).toContain(
        'file: "tests/babel/fixtures/basicFunctionCall.ts"'
      );
      expect(out).toContain('fn: "AgentChatScreen"');
      expect(out).toContain('fnPath: "AgentChatScreen"');
      expect(out).toMatch(/line:\s*4/);
      expect(out).toContain(".info('agent.chat.load')");
    });
  });

  describe('given a chained call (Logger.withError(err).error(...))', () => {
    it('when transformed, then the context is injected at the root of the chain, before .withError', () => {
      const out = transformFixture('chainedCall.ts');

      expect(out).toMatch(
        /Logger\.withContext\(\{[\s\S]*?\}\)\.withError\(err\)\.error\('agent\.chat\.fail'\)/
      );
      expect(out).toContain('fn: "handleFail"');
      expect(out).toContain('fnPath: "handleFail"');
    });
  });

  describe('given Logger.info(...) is called inside an arrow function assigned to a const', () => {
    it('when transformed, then fn and fnPath resolve to the const name', () => {
      const out = transformFixture('arrowConst.ts');

      expect(out).toContain('fn: "onPress"');
      expect(out).toContain('fnPath: "onPress"');
    });
  });

  describe('given Logger.info(...) is called at module scope (no enclosing function)', () => {
    it('when transformed, then fn and fnPath both fall back to null', () => {
      const out = transformFixture('moduleScope.ts');

      expect(out).toContain('fn: null');
      expect(out).toContain('fnPath: null');
      expect(out).toMatch(/line:\s*3/);
    });
  });

  describe('given Logger.info(...) is called inside a class method', () => {
    it('when transformed, then fn and fnPath resolve to the method name', () => {
      const out = transformFixture('classMethod.ts');

      expect(out).toContain('fn: "handleSend"');
      expect(out).toContain('fnPath: "handleSend"');
      expect(out).toMatch(/line:\s*5/);
    });
  });

  describe('given Logger.info(...) is called inside an arrow-function class property', () => {
    it('when transformed, then fn and fnPath resolve to the class property name', () => {
      const out = transformFixture('classPropertyArrow.ts');

      expect(out).toContain('fn: "handleRetry"');
      expect(out).toContain('fnPath: "handleRetry"');
      expect(out).toMatch(/line:\s*5/);
    });
  });

  describe('given Logger.info(...) is called inside an object method shorthand', () => {
    it('when transformed, then fn and fnPath resolve to the object property name', () => {
      const out = transformFixture('objectMethodShorthand.ts');

      expect(out).toContain('fn: "onSubmit"');
      expect(out).toContain('fnPath: "onSubmit"');
      expect(out).toMatch(/line:\s*5/);
    });
  });

  describe('given Logger.info(...) is called inside a function nested inside another function', () => {
    it('when transformed, then fn is the INNERMOST function and fnPath is the full outer-to-inner chain', () => {
      const out = transformFixture('nestedFunctions.ts');

      expect(out).toContain('fn: "handleRetry"');
      expect(out).not.toContain('fn: "AgentChatScreen"');
      expect(out).toContain('fnPath: "AgentChatScreen > handleRetry"');
    });
  });

  describe('given Logger.info(...) is called 3 levels deep (function > function > function)', () => {
    it('when transformed, then fnPath lists every lexical ancestor outer-to-inner', () => {
      const out = transformFixture('deeplyNestedFunctions.ts');

      expect(out).toContain('fn: "innerCallback"');
      expect(out).toContain(
        'fnPath: "OuterScreen > middleHandler > innerCallback"'
      );
    });
  });

  describe('given Logger.info(...) is called inside a function nested inside a class method', () => {
    it('when transformed, then fnPath chains the class method name with the nested function name', () => {
      const out = transformFixture('nestedInsideClassMethod.ts');

      expect(out).toContain('fn: "validate"');
      expect(out).toContain('fnPath: "handleSubmit > validate"');
    });
  });

  describe('given an unnamed intermediate function in the lexical chain (e.g. an inline callback arg)', () => {
    it('when transformed, then the unresolvable segment is rendered as <anonymous> without breaking the chain', () => {
      const out = transformFixture('anonymousIntermediateFunction.ts');

      expect(out).toContain('fn: "onTimeout"');
      expect(out).toContain(
        'fnPath: "ScreenWithEffect > <anonymous> > onTimeout"'
      );
    });
  });

  describe('given a local variable named Logger shadows the import', () => {
    it('when transformed, then the shadowed local is left untouched (no withContext injected)', () => {
      const out = transformFixture('shadowedLocal.ts');

      expect(out).not.toContain('withContext');
    });
  });

  describe('given Logger is referenced but never called (e.g. a re-export)', () => {
    it('when transformed, then it is left untouched', () => {
      const out = transformFixture('reExport.ts');

      expect(out).not.toContain('withContext');
      expect(out).toContain('export { Logger }');
    });
  });

  describe('given a file calls Logger.info(...) with no Logger import or declaration at all', () => {
    it('when transformed, then it is left untouched (no binding to resolve)', () => {
      const out = transformFixture('unrelatedLoggerNoImport.ts');

      expect(out).not.toContain('withContext');
    });
  });

  describe('given Logger is imported under an alias (import { Logger as Log })', () => {
    it('when transformed, then the aliased call site is left untouched — documented limitation, not a bug', () => {
      const out = transformFixture('aliasedImport.ts');

      expect(out).not.toContain('withContext');
      expect(out).toContain("Log.info('agent.chat.load')");
    });
  });

  describe('given the enclosing function destructures `testID` directly', () => {
    it('when transformed, then testID is injected as a bare identifier reference', () => {
      const out = transformFixture('testIdDestructuredParam.ts');

      expect(out).toContain('testID: testID');
    });
  });

  describe('given the enclosing function only has a `props` rest binding', () => {
    it('when transformed, then testID is injected as a props.testID member expression', () => {
      const out = transformFixture('testIdPropsRestParam.ts');

      expect(out).toContain('testID: props.testID');
    });
  });

  describe('given neither `testID` nor `props` are in scope', () => {
    it('when transformed, then testID falls back to null', () => {
      const out = transformFixture('testIdNeitherBindingInScope.ts');

      expect(out).toContain('testID: null');
    });
  });

  describe('given the props-rest shape in a file with no relation to design-system', () => {
    it('when transformed, then testID is still injected — this is a general rule, not gated by path', () => {
      const out = transformFixture(
        'testIdPropsRestParamAnyFile.ts',
        'features/leetcode/ui/ScreenLike.tsx'
      );

      expect(out).toContain('testID: props.testID');
    });
  });
});
