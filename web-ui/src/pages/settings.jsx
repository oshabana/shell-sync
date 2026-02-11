import { useState, useEffect } from 'preact/hooks';
import { get, post } from '../api';

export function Settings() {
  const [health, setHealth] = useState(null);
  const [syncing, setSyncing] = useState(false);
  const [message, setMessage] = useState(null);

  useEffect(() => {
    get('/health').then(setHealth).catch(() => null);
  }, []);

  const forceGitSync = async () => {
    setSyncing(true);
    setMessage(null);
    try {
      await post('/git/sync');
      setMessage('Git backup completed successfully.');
    } catch (err) {
      setMessage(`Error: ${err.message}`);
    }
    setSyncing(false);
  };

  return (
    <div>
      <h2>Settings</h2>

      <div class="card" style="margin-bottom:16px">
        <div class="label">Server Info</div>
        <div style="margin-top:8px;font-size:14px">
          <div>Status: <span class="status"><span class={`dot ${health ? 'green' : 'red'}`}></span> {health ? 'Online' : 'Offline'}</span></div>
          <div style="margin-top:4px">Active WebSocket connections: {health?.active_machines ?? '-'}</div>
        </div>
      </div>

      <div class="card" style="margin-bottom:16px">
        <div class="label">Git Backup</div>
        <div style="margin-top:8px">
          <button onClick={forceGitSync} disabled={syncing}>
            {syncing ? 'Syncing...' : 'Force Git Backup'}
          </button>
        </div>
        {message && <div style="margin-top:8px;font-size:14px">{message}</div>}
      </div>

      <div class="card">
        <div class="label">About</div>
        <div style="margin-top:8px;font-size:14px;color:var(--muted)">
          <div>Shell Sync v0.1.0</div>
          <div>Single binary server + client with mDNS auto-discovery</div>
        </div>
      </div>
    </div>
  );
}
