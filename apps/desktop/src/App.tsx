import { useEffect, useState } from 'react';
import { appVersion } from './lib/commands';

type VersionState =
  { status: 'loading' } | { status: 'ready'; version: string } | { status: 'error' };

export function App() {
  const [version, setVersion] = useState<VersionState>({ status: 'loading' });

  useEffect(() => {
    let cancelled = false;
    appVersion().then(
      (value) => {
        if (!cancelled) setVersion({ status: 'ready', version: value });
      },
      () => {
        if (!cancelled) setVersion({ status: 'error' });
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <main>
      <h1>Re-Open</h1>
      <p>
        {version.status === 'loading' && 'Loading version…'}
        {version.status === 'ready' && `Version ${version.version}`}
        {version.status === 'error' && 'Version unavailable'}
      </p>
    </main>
  );
}
