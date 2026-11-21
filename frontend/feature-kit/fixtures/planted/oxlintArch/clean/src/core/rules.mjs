import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
export const rules = [join, fileURLToPath, randomUUID];
