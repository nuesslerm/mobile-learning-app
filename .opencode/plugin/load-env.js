// Auto-load remote-agent/.env into process.env at opencode startup.
// Also injects via shell.env hook for every bash subprocess.
// Auto-discovered by opencode (plugin/ dir).

import { readFileSync } from 'fs';
import { resolve } from 'path';

let parsed = null;

function parseEnvFile(envPath) {
  const vars = {};
  try {
    const content = readFileSync(envPath, 'utf-8');
    for (const line of content.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith('#')) continue;
      const eqIdx = trimmed.indexOf('=');
      if (eqIdx === -1) continue;
      const key = trimmed.slice(0, eqIdx).trim();
      const val = trimmed.slice(eqIdx + 1).trim();
      if (key && val) vars[key] = val;
    }
  } catch { /* .env may not exist */ }
  return vars;
}

export default async ({ directory }) => {
  const envPath = resolve(directory, 'remote-agent/.env');
  parsed = parseEnvFile(envPath);

  // Set into process.env so providers, MCP servers, etc. inherit them
  for (const [key, val] of Object.entries(parsed)) {
    if (!process.env[key]) {
      process.env[key] = val;
    }
  }

  return {
    'shell.env': (env) => {
      // Ensure every bash call also gets these vars
      for (const [key, val] of Object.entries(parsed)) {
        if (!(key in env)) {
          env[key] = val;
        }
      }
    },
  };
};
