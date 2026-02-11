import { useState, useEffect } from 'preact/hooks';
import { get, post } from '../api';

export function Conflicts() {
  const [conflicts, setConflicts] = useState([]);
  const [error, setError] = useState(null);

  const load = () => {
    get('/conflicts').then(d => setConflicts(d.conflicts || [])).catch(e => setError(e.message));
  };

  useEffect(load, []);

  const resolve = async (id, resolution) => {
    try {
      await post('/conflicts/resolve', { conflict_id: id, resolution });
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <div>
      <h2>Conflicts</h2>
      {error && <div class="error">{error}</div>}
      {conflicts.length === 0 ? (
        <div class="empty">No conflicts. Everything is in sync.</div>
      ) : (
        <table>
          <thead>
            <tr><th>Alias</th><th>Group</th><th>Local</th><th>Remote</th><th>Actions</th></tr>
          </thead>
          <tbody>
            {conflicts.map(c => (
              <tr key={c.id}>
                <td><strong>{c.alias_name}</strong></td>
                <td><span class="badge yellow">{c.group_name}</span></td>
                <td><code>{c.local_command}</code></td>
                <td><code>{c.remote_command}</code></td>
                <td>
                  <button onClick={() => resolve(c.id, 'keep_local')} style="margin-right:4px">Keep Local</button>
                  <button class="secondary" onClick={() => resolve(c.id, 'keep_remote')}>Keep Remote</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
