export const api = async (path, options = {}) => {
  const response = await fetch(path, {
    ...options,
    headers: { "Content-Type": "application/json", ...(options.headers || {}) },
  });
  if (response.status === 409) {
    const err = new Error((await response.text()) || "revision conflict");
    err.status = 409;
    try { err.view = JSON.parse(err.message); } catch (_) { /* text body */ }
    throw err;
  }
  if (!response.ok) throw new Error((await response.text()) || `HTTP ${response.status}`);
  return response.status === 204 ? null : response.json();
};

export const newStrokeId = () =>
  (crypto.randomUUID && crypto.randomUUID()) ||
  `s-${Date.now()}-${Math.random().toString(16).slice(2)}`;
