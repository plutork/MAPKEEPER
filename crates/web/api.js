export const api = async (path, options = {}) => {
  const { onTiming, ...fetchOptions } = options;
  const started = performance.now();
  const response = await fetch(path, {
    ...fetchOptions,
    headers: { "Content-Type": "application/json", ...(fetchOptions.headers || {}) },
  });
  const headersMs = performance.now() - started;
  const bodyStarted = performance.now();
  const text = response.status === 204 ? "" : await response.text();
  const bodyMs = performance.now() - bodyStarted;
  const parseStarted = performance.now();
  let parsed = null;
  if (text) {
    try { parsed = JSON.parse(text); } catch (_) { /* plain text */ }
  }
  const parseMs = performance.now() - parseStarted;
  onTiming?.({
    path,
    status: response.status,
    headers_ms: headersMs,
    body_ms: bodyMs,
    parse_ms: parseMs,
    response_bytes: new TextEncoder().encode(text).byteLength,
    total_ms: performance.now() - started,
  });
  if (response.status === 409) {
    const err = new Error(text || "revision conflict");
    err.status = 409;
    if (parsed) err.view = parsed;
    throw err;
  }
  if (!response.ok) throw new Error(text || `HTTP ${response.status}`);
  return response.status === 204 ? null : parsed;
};

export const newStrokeId = () =>
  (crypto.randomUUID && crypto.randomUUID()) ||
  `s-${Date.now()}-${Math.random().toString(16).slice(2)}`;
