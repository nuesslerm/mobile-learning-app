import AsyncStorage from '@react-native-async-storage/async-storage';

import type { CodeDraftPort } from '../../domain/ports';

const keyOf = (problemId: string, language: string) => `leetcode-draft:${problemId}:${language}`;
const writes = new Map<string, Promise<void>>();

export const deviceCodeDrafts: CodeDraftPort = {
  async read(problemId, language) {
    const key = keyOf(problemId, language);
    const pending = writes.get(key);
    if (pending) await pending;
    return AsyncStorage.getItem(key);
  },

  write(problemId, language, code) {
    const key = keyOf(problemId, language);
    const persist = () => code === null ? AsyncStorage.removeItem(key) : AsyncStorage.setItem(key, code);
    const previous = writes.get(key);
    const write = previous ? previous.then(persist, persist) : Promise.resolve().then(persist);
    writes.set(key, write);
    const finished = () => {
      if (writes.get(key) === write) writes.delete(key);
    };
    void write.then(finished, finished);
    return write;
  },
};
