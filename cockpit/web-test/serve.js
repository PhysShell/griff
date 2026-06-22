// A minimal static file server for the prebuilt cockpit/dist. wasm-bindgen's
// glue streams the .wasm, so the `application/wasm` MIME matters (otherwise the
// browser warns and falls back to a slower array-buffer instantiate).
import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, resolve, relative } from 'node:path';

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
};

/** Serve `root` on an ephemeral port; resolves with the listening server. */
export function startServer(root) {
  const rootAbs = resolve(root);
  const server = http.createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://localhost');
      let path = decodeURIComponent(url.pathname);
      if (path.endsWith('/')) path += 'index.html';
      // Resolve under root and reject anything that escapes it — a real
      // containment check, not a string prefix (`/dist2` vs `/dist`) (#98 review).
      const file = resolve(rootAbs, `.${path}`);
      if (relative(rootAbs, file).startsWith('..')) {
        res.writeHead(403).end('forbidden');
        return;
      }
      const body = await readFile(file);
      res.writeHead(200, { 'content-type': MIME[extname(file)] ?? 'application/octet-stream' });
      res.end(body);
    } catch {
      res.writeHead(404).end('not found');
    }
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}
