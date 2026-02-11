import { useState, useEffect } from 'preact/hooks';

const LOCAL_API = 'http://127.0.0.1:18888';

async function fetchStats(last = '30d') {
  const res = await fetch(`${LOCAL_API}/api/local/stats?last=${last}`);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}

function BarChart({ items, label }) {
  if (!items || items.length === 0) return null;
  const max = Math.max(...items.map(i => i[1]), 1);
  return (
    <div class="stat-section">
      <h3>{label}</h3>
      <div class="bar-chart">
        {items.map(([name, count]) => (
          <div class="bar-row" key={name}>
            <span class="bar-label" title={name}>
              {name.length > 30 ? name.slice(0, 27) + '...' : name}
            </span>
            <div class="bar-track">
              <div
                class="bar-fill"
                style={{ width: `${(count / max) * 100}%` }}
              />
            </div>
            <span class="bar-count">{count}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function HourlyChart({ data }) {
  if (!data || data.length === 0) return null;
  const max = Math.max(...data, 1);
  return (
    <div class="stat-section">
      <h3>Activity by Hour</h3>
      <div class="hourly-chart">
        {data.map((count, hour) => (
          <div class="hourly-bar" key={hour} title={`${hour}:00 - ${count} commands`}>
            <div
              class="hourly-fill"
              style={{ height: `${(count / max) * 100}%` }}
            />
            <span class="hourly-label">{hour % 6 === 0 ? `${hour}` : ''}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function DailyChart({ data }) {
  if (!data || data.length === 0) return null;
  const days = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
  const max = Math.max(...data, 1);
  return (
    <div class="stat-section">
      <h3>Activity by Day</h3>
      <div class="bar-chart">
        {data.map((count, i) => (
          <div class="bar-row" key={days[i]}>
            <span class="bar-label">{days[i]}</span>
            <div class="bar-track">
              <div
                class="bar-fill"
                style={{ width: `${(count / max) * 100}%` }}
              />
            </div>
            <span class="bar-count">{count}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function Stats() {
  const [stats, setStats] = useState(null);
  const [error, setError] = useState(null);
  const [period, setPeriod] = useState('30d');
  const [loading, setLoading] = useState(true);

  const load = (p) => {
    setLoading(true);
    setError(null);
    fetchStats(p)
      .then(setStats)
      .catch(e => setError(e.message))
      .finally(() => setLoading(false));
  };

  useEffect(() => load(period), [period]);

  return (
    <div>
      <h2>Shell Stats</h2>
      <div class="form-row" style={{ marginBottom: '1rem' }}>
        {['7d', '30d', '90d', '1y', 'all'].map(p => (
          <button
            key={p}
            class={period === p ? '' : 'secondary'}
            onClick={() => setPeriod(p)}
          >
            {p === 'all' ? 'All Time' : p}
          </button>
        ))}
      </div>

      {error && (
        <div class="error">
          Could not reach local daemon. Make sure the daemon is running (shell-sync connect --foreground).
          <br /><small>{error}</small>
        </div>
      )}

      {loading && !error && <div class="empty">Loading stats...</div>}

      {stats && !loading && (
        <div>
          <div class="cards">
            <div class="card">
              <div class="label">Total Commands</div>
              <div class="value">{stats.total_commands.toLocaleString()}</div>
            </div>
            <div class="card">
              <div class="label">Unique Commands</div>
              <div class="value">{stats.unique_commands.toLocaleString()}</div>
            </div>
            <div class="card">
              <div class="label">Success Rate</div>
              <div class="value">{stats.success_rate.toFixed(1)}%</div>
            </div>
            <div class="card">
              <div class="label">Streak</div>
              <div class="value">{stats.streak_days} day{stats.streak_days !== 1 ? 's' : ''}</div>
            </div>
          </div>

          <div class="cards" style={{ marginTop: '0.5rem' }}>
            <div class="card">
              <div class="label">Avg Duration</div>
              <div class="value">{Math.round(stats.avg_duration_ms)} ms</div>
            </div>
            <div class="card">
              <div class="label">Median Duration</div>
              <div class="value">{stats.median_duration_ms} ms</div>
            </div>
            <div class="card">
              <div class="label">P95 Duration</div>
              <div class="value">{stats.p95_duration_ms} ms</div>
            </div>
          </div>

          <BarChart items={stats.top_commands} label="Top Commands" />
          <BarChart items={stats.top_prefixes} label="Top Prefixes" />
          <HourlyChart data={stats.hourly_distribution} />
          <DailyChart data={stats.daily_distribution} />
          <BarChart items={stats.per_directory} label="Top Directories" />
          {stats.per_machine.length > 1 && (
            <BarChart items={stats.per_machine} label="Per Machine" />
          )}
        </div>
      )}

      <style>{`
        .stat-section {
          margin-top: 1.5rem;
        }
        .stat-section h3 {
          margin-bottom: 0.5rem;
          font-size: 0.95rem;
          opacity: 0.8;
        }
        .bar-chart {
          display: flex;
          flex-direction: column;
          gap: 4px;
        }
        .bar-row {
          display: flex;
          align-items: center;
          gap: 8px;
        }
        .bar-label {
          width: 200px;
          font-size: 0.85rem;
          text-align: right;
          overflow: hidden;
          text-overflow: ellipsis;
          white-space: nowrap;
          font-family: monospace;
        }
        .bar-track {
          flex: 1;
          height: 18px;
          background: var(--bg-secondary, #2a2a2a);
          border-radius: 3px;
          overflow: hidden;
        }
        .bar-fill {
          height: 100%;
          background: var(--accent, #4fc3f7);
          border-radius: 3px;
          min-width: 2px;
          transition: width 0.3s ease;
        }
        .bar-count {
          width: 50px;
          text-align: right;
          font-size: 0.85rem;
          font-family: monospace;
          opacity: 0.7;
        }
        .hourly-chart {
          display: flex;
          align-items: flex-end;
          gap: 2px;
          height: 100px;
          padding-top: 8px;
        }
        .hourly-bar {
          flex: 1;
          display: flex;
          flex-direction: column;
          align-items: center;
          height: 100%;
          justify-content: flex-end;
        }
        .hourly-fill {
          width: 100%;
          background: var(--accent, #4fc3f7);
          border-radius: 2px 2px 0 0;
          min-height: 1px;
          transition: height 0.3s ease;
        }
        .hourly-label {
          font-size: 0.7rem;
          opacity: 0.5;
          margin-top: 4px;
          height: 14px;
        }
      `}</style>
    </div>
  );
}
