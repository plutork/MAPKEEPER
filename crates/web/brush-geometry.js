/** Pure brush disk / hex-line helpers (no DOM). */
import { probe_disk_cells, probe_hex_distance } from "./wasm-api.js";

export const diskCellsAt = (q, r, radius, width, height) => {
  const flat = probe_disk_cells(q, r, radius, width, height);
  const cells = [];
  for (let i = 0; i + 1 < flat.length; i += 2) cells.push({ q: flat[i], r: flat[i + 1] });
  return cells;
};

export const hexLine = (a, b) => {
  const n = probe_hex_distance(a.q, a.r, b.q, b.r);
  if (n === 0) return [{ q: a.q, r: a.r }];
  const out = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const x = a.q + (b.q - a.q) * t;
    const z = a.r + (b.r - a.r) * t;
    const y = -a.q - a.r + (-b.q - b.r - (-a.q - a.r)) * t;
    let rx = Math.round(x);
    let ry = Math.round(y);
    let rz = Math.round(z);
    const xDiff = Math.abs(rx - x);
    const yDiff = Math.abs(ry - y);
    const zDiff = Math.abs(rz - z);
    if (xDiff > yDiff && xDiff > zDiff) rx = -ry - rz;
    else if (yDiff > zDiff) ry = -rx - rz;
    else rz = -rx - ry;
    out.push({ q: rx, r: rz });
  }
  return out;
};
