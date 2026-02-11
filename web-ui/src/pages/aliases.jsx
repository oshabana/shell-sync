import { useState, useEffect } from 'preact/hooks';
import { get, post, del } from '../api';

export function Aliases() {
  const [aliases, setAliases] = useState([]);
  const [error, setError] = useState(null);
  const [name, setName] = useState('');
  const [command, setCommand] = useState('');
  const [group, setGroup] = useState('default');

  const load = () => {
    get('/aliases').then(d => setAliases(d.aliases || [])).catch(e => setError(e.message));
  };

  useEffect(load, []);

  const handleAdd = async (e) => {
    e.preventDefault();
    setError(null);
    try {
      await post('/aliases', { name, command, group });
      setName('');
      setCommand('');
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  const handleDelete = async (id) => {
    try {
      await del(`/aliases/${id}`);
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <div>
      <h2>Aliases</h2>
      {error && <div class="error">{error}</div>}

      <form onSubmit={handleAdd}>
        <div class="form-row">
          <input placeholder="Name" value={name} onInput={e => setName(e.target.value)} required style="width:140px" />
          <input placeholder="Command" value={command} onInput={e => setCommand(e.target.value)} required style="flex:1;min-width:200px" />
          <input placeholder="Group" value={group} onInput={e => setGroup(e.target.value)} style="width:120px" />
          <button type="submit">Add</button>
        </div>
      </form>

      {aliases.length === 0 ? (
        <div class="empty">No aliases yet. Add one above.</div>
      ) : (
        <table>
          <thead>
            <tr><th>Name</th><th>Command</th><th>Group</th><th>Version</th><th></th></tr>
          </thead>
          <tbody>
            {aliases.map(a => (
              <tr key={a.id}>
                <td><strong>{a.name}</strong></td>
                <td><code>{a.command}</code></td>
                <td><span class="badge green">{a.group_name}</span></td>
                <td>{a.version}</td>
                <td><button class="danger" onClick={() => handleDelete(a.id)}>Delete</button></td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
