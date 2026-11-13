import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { parse } from 'yaml';

const read = (file) => parse(readFileSync(file, 'utf8'), { merge: true });

// A compose file as docker compose reads it, with each service's `extends` resolved against the file it names.
export function loadCompose(file) {
  const compose = read(file);
  for (const service of Object.values(compose.services)) {
    if (!service.extends) continue;
    const base = read(resolve(dirname(file), service.extends.file)).services[service.extends.service];
    const { extends: _, ...own } = service;
    Object.assign(service, { ...base, ...own });
    delete service.extends;
  }
  return compose;
}
