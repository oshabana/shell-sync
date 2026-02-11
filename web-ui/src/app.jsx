import { useState, useEffect } from 'preact/hooks';
import { Dashboard } from './pages/dashboard';
import { Aliases } from './pages/aliases';
import { Machines } from './pages/machines';
import { Conflicts } from './pages/conflicts';
import { History } from './pages/history';
import { Settings } from './pages/settings';

const PAGES = {
  '': { label: 'Dashboard', component: Dashboard },
  aliases: { label: 'Aliases', component: Aliases },
  machines: { label: 'Machines', component: Machines },
  conflicts: { label: 'Conflicts', component: Conflicts },
  history: { label: 'History', component: History },
  settings: { label: 'Settings', component: Settings },
};

function useHash() {
  const [hash, setHash] = useState(location.hash.slice(1) || '');
  useEffect(() => {
    const onHash = () => setHash(location.hash.slice(1) || '');
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);
  return hash;
}

export function App() {
  const page = useHash();
  const current = PAGES[page] || PAGES[''];
  const Page = current.component;

  return (
    <div class="layout">
      <aside class="sidebar">
        <h1>Shell Sync</h1>
        <nav>
          {Object.entries(PAGES).map(([key, { label }]) => (
            <a
              key={key}
              href={`#${key}`}
              class={page === key ? 'active' : ''}
            >
              {label}
            </a>
          ))}
        </nav>
      </aside>
      <main class="main">
        <Page />
      </main>
    </div>
  );
}
