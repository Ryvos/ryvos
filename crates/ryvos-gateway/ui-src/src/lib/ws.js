import { writable } from 'svelte/store';

export const connectionStatus = writable('disconnected');
export const activityFeed = writable([]);
export const streamingText = writable('');

let ws = null;
let reconnectTimer = null;
let retryCount = 0;
const MAX_RETRY_DELAY = 30000;

export function connect(apiKey) {
  if (ws) ws.close();
  clearTimeout(reconnectTimer);

  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const url = `${proto}//${location.host}/ws${apiKey ? `?token=${encodeURIComponent(apiKey)}` : ''}`;

  try {
    ws = new WebSocket(url);
  } catch (e) {
    console.error('WebSocket creation failed:', e);
    connectionStatus.set('error');
    return;
  }

  ws.onopen = () => {
    connectionStatus.set('connected');
    retryCount = 0;
  };

  ws.onclose = () => {
    connectionStatus.set('disconnected');
    const delay = Math.min(1000 * Math.pow(2, retryCount), MAX_RETRY_DELAY);
    retryCount++;
    reconnectTimer = setTimeout(() => connect(apiKey), delay);
  };

  ws.onerror = () => connectionStatus.set('error');

  ws.onmessage = (e) => {
    try {
      const msg = JSON.parse(e.data);
      if (msg.type === 'event') {
        const event = msg.event || {};
        if (event.kind === 'text_delta') {
          streamingText.update(t => t + (event.text || ''));
        }
        if (event.kind !== 'text_delta') {
          activityFeed.update(feed => {
            const entry = {
              time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
              kind: event.kind,
              text: event.text || event.tool || '',
              session: msg.session_id ? msg.session_id.substring(0, 10) : '',
              detail: formatEventDetail(event),
              data: event.data,
            };
            return [entry, ...feed].slice(0, 100);
          });
        }
      }
    } catch (e) {
      console.warn('Failed to parse WebSocket message:', e);
    }
  };
}

function formatEventDetail(evt) {
  switch (evt.kind) {
    case 'run_started': return 'Run started';
    case 'run_complete': return `Completed in ${evt.data?.total_turns || '?'} turns`;
    case 'run_error': return `Error: ${evt.data?.error || '?'}`;
    case 'tool_start': return `Tool: ${evt.tool || '?'}`;
    case 'tool_end': return `Tool done: ${evt.tool || '?'}`;
    case 'usage_update': return `+${evt.data?.input_tokens || 0} in / +${evt.data?.output_tokens || 0} out`;
    case 'budget_warning': return `Budget at ${evt.data?.utilization_pct || '?'}%`;
    case 'budget_exceeded': return 'Budget exceeded!';
    case 'heartbeat_fired': return 'Heartbeat fired';
    case 'heartbeat_ok': return `Heartbeat OK (${evt.data?.response_chars || 0} chars)`;
    case 'heartbeat_alert': return `Heartbeat ALERT: ${(evt.data?.message || '').slice(0, 60)}`;
    case 'cron_fired': return `Cron: ${evt.data?.job_name || '?'}`;
    case 'cron_complete': return `Cron done: ${evt.data?.job_name || '?'}`;
    case 'guardian_stall': return 'Guardian: stall detected';
    case 'guardian_doom_loop': return 'Guardian: doom loop detected';
    case 'guardian_budget_alert': return 'Guardian: budget alert';
    case 'approval_requested': return `Approval needed: ${evt.data?.tool_name || '?'}`;
    case 'tool_blocked': return `Blocked: ${evt.tool || '?'}`;
    default: return evt.kind || '';
  }
}

export function disconnect() {
  clearTimeout(reconnectTimer);
  if (ws) ws.close();
  ws = null;
  connectionStatus.set('disconnected');
}

export function resetStreamingText() {
  streamingText.set('');
}

export function sendWs(method, params = {}) {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  ws.send(JSON.stringify({
    type: 'request',
    id: Date.now().toString(),
    method,
    params,
  }));
}
