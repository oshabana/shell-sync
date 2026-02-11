const BASE = '';

export async function api(path, opts = {}) {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json', ...opts.headers },
    ...opts,
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `HTTP ${res.status}`);
  }
  return res.json();
}

export const get = (path) => api(`/api${path}`);
export const post = (path, body) => api(`/api${path}`, { method: 'POST', body: JSON.stringify(body) });
export const put = (path, body) => api(`/api${path}`, { method: 'PUT', body: JSON.stringify(body) });
export const del = (path) => api(`/api${path}`, { method: 'DELETE' });
