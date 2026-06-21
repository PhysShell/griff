// Shared helpers for the cockpit browser tests: launch flags, the signature
// palette, PNG sampling, and a fresh-page boot that focuses the canvas so
// keyboard events reach eframe.
import { PNG } from 'pngjs';

// Headless Chromium gets WebGL from SwiftShader (software) — no GPU needed.
export const LAUNCH_ARGS = [
  '--ignore-gpu-blocklist',
  '--use-gl=angle',
  '--use-angle=swiftshader',
  '--enable-unsafe-swiftshader',
];

// Signature solid fills from cockpit/src/lib.rs (role -> colour). Flat fills
// match near-exactly; anti-aliased edges are noise.
export const SIGNATURE = {
  'note lane-0 (orange)': [0xff, 0x7a, 0x45],
  'Riff band (blue)': [0x16, 0x68, 0xdc],
  'Breakdown band (red)': [0xcf, 0x13, 0x22],
  'playhead (yellow)': [0xff, 0xcf, 0x4d],
};
export const BG = [0x1b, 0x1b, 0x1f]; // index.html body background

export const near = (px, [r, g, b], tol = 12) =>
  Math.abs(px[0] - r) <= tol && Math.abs(px[1] - g) <= tol && Math.abs(px[2] - b) <= tol;

export const decode = (buf) => PNG.sync.read(buf);
export const canvasShot = (page) => page.locator('canvas').screenshot();

/** Boot the cockpit on a fresh page; resolves once the wasm has painted. */
export async function bootPage(browser, baseURL, { width = 1100, height = 520 } = {}) {
  const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 2 });
  const errors = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(`console.error: ${m.text()}`);
  });
  await page.goto(baseURL, { waitUntil: 'load' });
  await page.waitForFunction(() => !document.getElementById('loading'), { timeout: 30000 });
  await page.waitForTimeout(2500); // initial paint
  await page.locator('canvas').click(); // focus the canvas so eframe gets keys
  return { page, errors };
}

/** Non-background fraction + per-signature-colour pixel counts. */
export function analyze(img) {
  const counts = Object.fromEntries(Object.keys(SIGNATURE).map((k) => [k, 0]));
  let nonBg = 0;
  for (let i = 0; i < img.data.length; i += 4) {
    const px = [img.data[i], img.data[i + 1], img.data[i + 2]];
    if (!near(px, BG, 16)) nonBg += 1;
    for (const [name, c] of Object.entries(SIGNATURE)) if (near(px, c)) counts[name] += 1;
  }
  return { counts, nonBg, total: img.width * img.height };
}

/** x (image px) of the tallest playhead-yellow column — its position, or -1. */
export function findPlayheadX(img) {
  const { width, height, data } = img;
  let best = -1;
  let bestCount = 0;
  for (let x = 0; x < width; x += 1) {
    let c = 0;
    for (let y = 0; y < height; y += 1) {
      const i = (y * width + x) * 4;
      if (near([data[i], data[i + 1], data[i + 2]], SIGNATURE['playhead (yellow)'])) c += 1;
    }
    if (c > bestCount) {
      bestCount = c;
      best = x;
    }
  }
  return bestCount > height * 0.3 ? best : -1; // require a tall column, not stray px
}

/** Fraction of pixels whose colour differs by more than `tol` on any channel. */
export function frameDiff(a, b, tol = 24) {
  const n = Math.min(a.data.length, b.data.length);
  let changed = 0;
  for (let i = 0; i < n; i += 4) {
    if (
      Math.abs(a.data[i] - b.data[i]) > tol ||
      Math.abs(a.data[i + 1] - b.data[i + 1]) > tol ||
      Math.abs(a.data[i + 2] - b.data[i + 2]) > tol
    ) {
      changed += 1;
    }
  }
  return changed / (n / 4);
}

/** Average [r,g,b] over a gx×gy block grid — a downscale for coarse compares. */
export function blockAverages(img, gx = 64, gy = 32) {
  const { width, height, data } = img;
  const blocks = [];
  for (let by = 0; by < gy; by += 1) {
    const y0 = Math.floor((by * height) / gy);
    const y1 = Math.floor(((by + 1) * height) / gy);
    for (let bx = 0; bx < gx; bx += 1) {
      const x0 = Math.floor((bx * width) / gx);
      const x1 = Math.floor(((bx + 1) * width) / gx);
      let r = 0;
      let g = 0;
      let b = 0;
      let n = 0;
      for (let y = y0; y < y1; y += 1) {
        for (let x = x0; x < x1; x += 1) {
          const i = (y * width + x) * 4;
          r += data[i];
          g += data[i + 1];
          b += data[i + 2];
          n += 1;
        }
      }
      blocks.push(n ? [r / n, g / n, b / n] : [0, 0, 0]);
    }
  }
  return blocks;
}

/** Fraction of downscaled blocks whose average colour matches within `tol`. */
export function coarseMatch(a, b, tol = 26) {
  const A = blockAverages(a);
  const B = blockAverages(b);
  let ok = 0;
  for (let i = 0; i < A.length; i += 1) {
    if (
      Math.abs(A[i][0] - B[i][0]) <= tol &&
      Math.abs(A[i][1] - B[i][1]) <= tol &&
      Math.abs(A[i][2] - B[i][2]) <= tol
    ) {
      ok += 1;
    }
  }
  return ok / A.length;
}

/** A copy of `live` with the blocks that drift from `ref` tinted red — a visual
 *  diff for the CI artifact, so a regression shows *where* it moved. */
export function diffImage(ref, live, tol = 26, gx = 64, gy = 32) {
  const { width, height } = live;
  const out = new PNG({ width, height });
  live.data.copy(out.data);
  const A = blockAverages(ref, gx, gy);
  const B = blockAverages(live, gx, gy);
  for (let by = 0; by < gy; by += 1) {
    for (let bx = 0; bx < gx; bx += 1) {
      const k = by * gx + bx;
      const drift =
        Math.abs(A[k][0] - B[k][0]) > tol ||
        Math.abs(A[k][1] - B[k][1]) > tol ||
        Math.abs(A[k][2] - B[k][2]) > tol;
      if (!drift) continue;
      const x0 = Math.floor((bx * width) / gx);
      const x1 = Math.floor(((bx + 1) * width) / gx);
      const y0 = Math.floor((by * height) / gy);
      const y1 = Math.floor(((by + 1) * height) / gy);
      for (let y = y0; y < y1; y += 1) {
        for (let x = x0; x < x1; x += 1) {
          const i = (y * width + x) * 4;
          out.data[i] = Math.min(255, out.data[i] + 130);
          out.data[i + 1] = Math.floor(out.data[i + 1] * 0.35);
          out.data[i + 2] = Math.floor(out.data[i + 2] * 0.35);
        }
      }
    }
  }
  return PNG.sync.write(out);
}

/** Count pixels near `color` (within `tol`) — for colours outside SIGNATURE. */
export function countColor(img, color, tol = 12) {
  let n = 0;
  for (let i = 0; i < img.data.length; i += 4) {
    if (near([img.data[i], img.data[i + 1], img.data[i + 2]], color, tol)) n += 1;
  }
  return n;
}
