import { useState, useEffect } from 'preact/hooks';
import { get } from '../api';

function formatTime(ms) {
  if (!ms) return '-';
  return new Date(ms).toLocaleString();
}

const ACTION_BADGE = {
  add: 'green',
  update: 'yellow',
  delete: 'red',
};

export function History() {
  const [history, setHistory] = useState([]);
  const [error, setError] = useState(null);

  useEffect(() => {
    get('/history?limit=200').then(d => setHistory(d.history || [])).catch(e => setError(e.message));
  }, []);

  return (
    <div>
      <h2>Sync History</h2>
      {error && <div class="error">{error}</div>}
      {history.length === 0 ? (
        <div class="empty">No history yet.</div>
      ) : (
        <table>
          <thead>
            <tr><th>Time</th><th>Action</th><th>Alias</th><th>Command</th><th>Group</th></tr>
          </thead>
          <tbody>
            {history.map(h => (
              <tr key={h.id}>
                <td>{formatTime(h.timestamp)}</td>
                <td><span class={`badge ${ACTION_BADGE[h.action] || ''}`}>{h.action}</span></td>
                <td><strong>{h.alias_name}</strong></td>
                <td><code>{h.alias_command || '-'}</code></td>
                <td>{h.group_name || '-'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
