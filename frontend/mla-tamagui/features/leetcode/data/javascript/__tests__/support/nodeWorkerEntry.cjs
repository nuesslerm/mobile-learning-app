// A worker_threads entry that gives a Node worker the shape of a browser's: postMessage and onmessage on the global scope, the
// script run as a classic script under a name (its stack frames carry it, as a blob url's do), and a failure of the script
// reported as the {message, line, column} an ErrorEvent gives.
const { parentPort, workerData } = require('node:worker_threads');
const vm = require('node:vm');

const { script, name } = workerData;

globalThis.postMessage = (message) => parentPort.postMessage(message);
parentPort.on('message', (data) => {
  if (typeof globalThis.onmessage === 'function') globalThis.onmessage({ data });
});

const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function position(stack) {
  const lines = stack.split('\n');
  const header = new RegExp(`^${escaped}:(\\d+)$`).exec(lines[0] || '');
  if (header) {
    const caret = (lines[2] || '').indexOf('^');
    return { line: Number(header[1]), column: caret >= 0 ? caret + 1 : undefined };
  }
  const frame = new RegExp(`${escaped}:(\\d+):(\\d+)`).exec(stack);
  return frame ? { line: Number(frame[1]), column: Number(frame[2]) } : {};
}

function normalized(error) {
  if (error === null || typeof error !== 'object') return { message: String(error) };
  return { message: `${error.name}: ${error.message}`, ...position(String(error.stack || '')) };
}

try {
  vm.runInThisContext(script, { filename: name });
} catch (error) {
  parentPort.postMessage({ __workerError: normalized(error) });
}
