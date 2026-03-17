let apiKey = localStorage.getItem('ryvos_api_key') || '';

export function setApiKey(key) {
  apiKey = key;
  localStorage.setItem('ryvos_api_key', key);
}

export function getApiKey() { return apiKey; }

export function clearApiKey() {
  apiKey = '';
  localStorage.removeItem('ryvos_api_key');
}

export async function apiFetch(path, options = {}) {
  const headers = { 'Content-Type': 'application/json', ...options.headers };
  if (apiKey) headers['Authorization'] = `Bearer ${apiKey}`;
  const res = await fetch(path, { ...options, headers });
  if (res.status === 401) {
    clearApiKey();
    window.location.reload();
    throw new Error('Unauthorized');
  }
  if (!res.ok) throw new Error(`API ${res.status}: ${await res.text()}`);
  return res.json();
}
