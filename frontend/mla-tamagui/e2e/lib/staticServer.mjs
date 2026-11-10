/**
 * Static file server for a web export, with the one behaviour a plain file
 * server lacks: an extensionless request falls back to `<path>.html`.
 *
 * Expo writes `showcase.html` and expo-router expects `/showcase` to reach it,
 * which is how GitHub Pages resolves it. Without the fallback the request 404s
 * and the app renders "Unmatched Route" -- indistinguishable from a real
 * routing bug in the app itself.
 */
import { createReadStream, existsSync, readdirSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';

const [, , root, port] = process.argv;

const CONTENT_TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.ttf': 'font/ttf',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
};

/**
 * An Expo static export writes a dynamic route to its LITERAL template path --
 * `leetcode/[id]/description.html`, not one file per id -- so a request for
 * `/leetcode/two-sum/description` matches nothing on disk and 404s. The app
 * then renders nothing at all, which reads as a broken screen rather than a
 * missing file (issue #164: an overlay route has to be addressable directly,
 * or a check can only reach it by replaying taps).
 *
 * A real host resolves this with a rewrite; this mirrors that by walking the
 * request path segment by segment and, when a segment has no directory of its
 * own, taking the single `[param]` sibling. Only ONE bracket directory per
 * level is ever accepted -- with two, the correct choice is the router's to
 * make, not a file server's, and guessing would hide a genuine ambiguity.
 */
function resolveDynamic(relative) {
  const segments = relative.split(/[/\\]/).filter(Boolean);
  let current = root;
  for (let i = 0; i < segments.length; i++) {
    const literal = join(current, segments[i]);
    const isLast = i === segments.length - 1;
    if (existsSync(literal)) {
      current = literal;
      continue;
    }
    if (isLast && existsSync(`${literal}.html`)) return `${literal}.html`;
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      return null;
    }
    // `[id]` (a directory of child routes) and `[id].html` (the route itself)
    // are ONE route, not two candidates -- compare on the param name so the
    // pair does not read as an ambiguity. Which of the two is wanted depends
    // on position: a trailing segment wants the page, an inner one wants the
    // directory holding its children.
    const brackets = entries.filter(
      (entry) => entry.name.startsWith('[') && entry.name.includes(']')
    );
    const params = new Set(
      brackets.map((entry) => entry.name.replace(/\.html$/, ''))
    );
    if (params.size !== 1) return null;
    const param = [...params][0];
    if (isLast) {
      const asFile = join(current, `${param}.html`);
      return existsSync(asFile) ? asFile : null;
    }
    const asDir = join(current, param);
    if (!existsSync(asDir)) return null;
    current = asDir;
  }
  const indexFile = join(current, 'index.html');
  if (existsSync(indexFile)) return indexFile;
  return existsSync(`${current}.html`) ? `${current}.html` : null;
}

function resolveFile(urlPath) {
  // `normalize` after stripping the leading slash keeps `..` from escaping the
  // served root -- a traversal here would read the repo, not just the export.
  const relative = normalize(decodeURIComponent(urlPath)).replace(/^(\.\.[/\\])+/, '');
  const base = join(root, relative);
  for (const candidate of [base, `${base}.html`, join(base, 'index.html')]) {
    if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
  }
  return resolveDynamic(relative);
}

createServer((req, res) => {
  const file = resolveFile(new URL(req.url, 'http://127.0.0.1').pathname);
  if (!file) {
    res.writeHead(404, { 'content-type': 'text/plain' });
    res.end('not found');
    return;
  }
  res.writeHead(200, { 'content-type': CONTENT_TYPES[extname(file)] ?? 'application/octet-stream' });
  createReadStream(file).pipe(res);
}).listen(Number(port), '127.0.0.1');
