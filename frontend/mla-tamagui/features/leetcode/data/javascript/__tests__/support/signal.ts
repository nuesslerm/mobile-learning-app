/** A queue a test waits on: `push` from a callback, `next` resolves with the oldest value not yet read. */
export function signal<T>() {
  const items: T[] = [];
  const waiters: ((value: T) => void)[] = [];
  return {
    push(value: T) {
      const waiter = waiters.shift();
      if (waiter) waiter(value);
      else items.push(value);
    },
    next: () =>
      new Promise<T>((resolve) => {
        if (items.length > 0) resolve(items.shift() as T);
        else waiters.push(resolve);
      }),
  };
}
