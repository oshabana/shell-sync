import { useState, useEffect } from 'preact/hooks';
import { get } from '../api';

function timeAgo(ms) {
  if (!ms) return 'never';
  const diff = Date.now() - ms;
  if (diff < 60000) return 'just now';
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
  return `${Math.floor(diff / 86400000)}d ago`;
}

export function Machines() {
  const [machines, setMachines] = useState([]);
  const [error, setError] = useState(null);

  useEffect(() => {
    get('/machines').then(d => setMachines(d.machines || [])).catch(e => setError(e.message));
  }, []);

  return (
    <div>
      <h2>Machines</h2>
      {error && <div class="error">{error}</div>}
      {machines.length === 0 ? (
        <div class="empty">No machines registered.</div>
      ) : (
        <table>
          <thead>
            <tr><th>Hostname</th><th>OS</th><th>Groups</th><th>Last Seen</th></tr>
          </thead>
          <tbody>
            {machines.map(m => (
              <tr key={m.machine_id}>
                <td><strong>{m.hostname}</strong></td>
                <td>{m.os_type || '-'}</td>
                <td>{(m.groups || []).map(g => <span class="badge green" style="margin-right:4px">{g}</span>)}</td>
                <td>{timeAgo(m.last_seen)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
