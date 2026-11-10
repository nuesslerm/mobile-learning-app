import { Logger } from '../../src';

export function topLevelFunction(): void {
  Logger.info('top-level-function'); // @site top-level-function fn=topLevelFunction
}

export const arrowConst = (): void => {
  Logger.info('arrow-const'); // @site arrow-const fn=arrowConst
};

export class Greeter {
  greet(): void {
    Logger.info('method'); // @site method fn=greet
  }
}

export function outer(): void {
  function inner(): void {
    Logger.info('nested-function'); // @site nested-function fn=inner
  }
  inner();
}

export function withCallback(): void {
  [1].forEach(() => {
    Logger.info('callback-in-function'); // @site callback-in-function fn=withCallback
  });
}

export async function asyncFunction(): Promise<void> {
  Logger.info('async-function'); // @site async-function fn=asyncFunction
}

Logger.info('module-level'); // @site module-level fn=-

export async function runAll(): Promise<void> {
  topLevelFunction();
  arrowConst();
  new Greeter().greet();
  outer();
  withCallback();
  await asyncFunction();
}
