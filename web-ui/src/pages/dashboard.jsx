import { useState, useEffect } from 'preact/hooks';
import { get } from '../api';

export function Dashboard() {
  const [health, setHealth] = useState(null);
  const [stats, setStats] = useState({ aliases: 0, machines: 0, conflicts: 0 });
  const [error, setError] = useState(null);

  useEffect(() => {
    Promise.all([
      get('/health').catch(() => null),
      get('/aliases').catch(() => ({ count: 0 })),
      get('/machines').catch(() => ({ count: 0 })),
      get('/conflicts').catch(() => ({ count: 0 })),
    ]).then(([h, a, m, c]) => {
      setHealth(h);
      setStats({ aliases: a?.count || 0, machines: m?.count || 0, conflicts: c?.count || 0 });
    }).catch(e => setError(e.message));
  }, []);

  return (
    <div>
      <h2>Dashboard</h2>
      {error && <div class="error">{error}</div>}
      <div class="cards">
        <div class="card">
          <div class="label">Status</div>
          <div class="value">
            <span class="status">
              <span class={`dot ${health ? 'green' : 'red'}`}></span>
              {health ? 'Online' : 'Checking...'}
            </span>
          </div>
        </div>
        <div class="card">
          <div class="label">Active Connections</div>
          <div class="value">{health?.active_machines ?? '-'}</div>
        </div>
        <div class="card">
          <div class="label">Total Aliases</div>
          <div class="value">{stats.aliases}</div>
        </div>
        <div class="card">
          <div class="label">Machines</div>
          <div class="value">{stats.machines}</div>
        </div>
        <div class="card">
          <div class="label">Conflicts</div>
          <div class="value">
            {stats.conflicts > 0
              ? <span class="badge red">{stats.conflicts}</span>
              : <span class="badge green">0</span>
            }
          </div>
        </div>
      </div>
    </div>
  );
}
