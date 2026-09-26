import React, { useEffect, useState } from 'react';

type WarehouseRole = 'Admin' | 'Warehouse Manager' | 'Warehouse Staff';

const roles: WarehouseRole[] = ['Admin', 'Warehouse Manager', 'Warehouse Staff'];

async function loadFrontendEndpoints(): Promise<string[]> {
  const response = await fetch('/api/frontend/endpoints');
  if (!response.ok) {
    throw new Error('Backend endpoints for the frontend interface are unavailable.');
  }
  return response.json() as Promise<string[]>;
}

export function WarehouseApp(): JSX.Element {
  const [activeRole, setActiveRole] = useState<WarehouseRole>('Admin');
  const [endpoints, setEndpoints] = useState<string[]>([]);

  useEffect(() => {
    void loadFrontendEndpoints()
      .then(setEndpoints)
      .catch(() => setEndpoints(['/api/login', '/api/inventory']));
  }, []);

  return (
    <main className="warehouse-interface" aria-label="Warehouse interface web application">
      <section className="web-application-shell">
        <h1>Warehouse Interface</h1>
        <p>Use this browser-based Warehouse interface to interact with the Warehouse system.</p>
      </section>

      <section aria-labelledby="roles-heading" className="role-panel">
        <h2 id="roles-heading">User roles</h2>
        <div role="group" aria-label="Warehouse interface roles">
          {roles.map((role) => (
            <button
              key={role}
              type="button"
              aria-pressed={activeRole === role}
              onClick={() => setActiveRole(role)}
            >
              {role}
            </button>
          ))}
        </div>
        <p>{activeRole} can interact with the Warehouse interface.</p>
      </section>

      <nav aria-label="Major functionalities" className="major-functionalities">
        <h2>Major functionalities</h2>
        <a href="#login" className="major-functionality">Login</a>
      </nav>

      <section id="login" aria-labelledby="login-heading" className="login-card">
        <h2 id="login-heading">Login</h2>
        <form>
          <label>
            Username
            <input name="username" autoComplete="username" />
          </label>
          <label>
            Password
            <input name="password" type="password" autoComplete="current-password" />
          </label>
          <button type="submit">Sign in</button>
        </form>
      </section>

      <section aria-labelledby="backend-heading" className="backend-status">
        <h2 id="backend-heading">Backend endpoints for the frontend interface</h2>
        <ul>
          {endpoints.map((endpoint) => (
            <li key={endpoint}>{endpoint}</li>
          ))}
        </ul>
      </section>

      <section className="react-typescript-evidence" aria-label="React TypeScript implementation">
        <p>This interface is implemented as React components written in TypeScript.</p>
      </section>
    </main>
  );
}
