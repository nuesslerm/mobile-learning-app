import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { transformFileAsync } from '@babel/core';

const require = createRequire(import.meta.url);
const injectLoggerSource = require('@lib/logging/babel').injectLoggerSource;

/**
 * Node ESM loader that lets TypeScript/Node apps run without tsx while still
 * applying the @lib/logging/babel plugin. Use with:
 *
 *   node --import @lib/logging/babel-register src/index.ts
 *
 * or, equivalently, register this loader directly via --loader.
 */

export async function load(url, context, nextLoad) {
  if (!url.startsWith('file:')) {
    return nextLoad(url, context);
  }
  const filePath = fileURLToPath(url);

  if (!/\.tsx?$/i.test(filePath)) {
    return nextLoad(url, context);
  }

  const result = await transformFileAsync(filePath, {
    presets: [require.resolve('@babel/preset-typescript')],
    plugins: [injectLoggerSource],
  });

  if (!result?.code) {
    throw new Error(`Babel transform failed for ${filePath}`);
  }

  return {
    format: 'module',
    source: result.code,
    shortCircuit: true,
  };
}

export async function resolve(specifier, context, nextResolve) {
  // Allow importing .ts files directly (e.g. import { x } from './foo.ts').
  if (/\.tsx?$/i.test(specifier)) {
    return nextResolve(specifier, context);
  }

  // Extensionless relative imports: try .ts, then directory/index.ts.
  if (specifier.startsWith('./') || specifier.startsWith('../')) {
    const hasExtension = /\.[^./]+$/i.test(specifier);
    if (!hasExtension) {
      try {
        return await nextResolve(`${specifier}.ts`, context);
      } catch {
        try {
          return await nextResolve(`${specifier}/index.ts`, context);
        } catch {
          // fall through to default resolution
        }
      }
    }
  }

  // Mirror tsx behavior: a specifier ending in .js may resolve to a .ts file.
  if (specifier.endsWith('.js')) {
    const tsSpecifier = specifier.slice(0, -3) + '.ts';
    try {
      return await nextResolve(tsSpecifier, context);
    } catch {
      // fall through to default resolution
    }
  }

  return nextResolve(specifier, context);
}
