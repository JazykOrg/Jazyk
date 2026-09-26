const assert = require('assert');
const http = require('http');
const fs = require('fs');
const path = require('path');

function loadBackendApp(root) {
  const candidates = [
    'server.js',
    'src/server.js',
    'backend/server.js',
    'backend/src/server.js',
    'app.js',
    'src/app.js',
    'backend/app.js',
    'backend/src/app.js'
  ];

  for (const candidate of candidates) {
    const fullPath = path.join(root, candidate);
    if (!fs.existsSync(fullPath)) continue;
    const exported = require(fullPath);
    const app = exported && (exported.app || exported.default || exported);
    if (app && typeof app.listen === 'function') {
      return { app, source: candidate };
    }
  }

  throw new Error('No backend application export with a listen() method was found in common server/app entrypoints');
}

function requestJson(server, method, endpointPath, payload) {
  const { port } = server.address();
  const body = payload ? JSON.stringify(payload) : '';
  return new Promise((resolve, reject) => {
    const request = http.request(
      {
        hostname: '127.0.0.1',
        port,
        path: endpointPath,
        method,
        timeout: 2000,
        headers: {
          'content-type': 'application/json',
          'content-length': Buffer.byteLength(body)
        }
      },
      (response) => {
        let responseBody = '';
        response.setEncoding('utf8');
        response.on('data', (chunk) => { responseBody += chunk; });
        response.on('end', () => {
          let parsed = null;
          try {
            parsed = responseBody ? JSON.parse(responseBody) : null;
          } catch (error) {
            reject(new Error(`${method} ${endpointPath} did not return JSON: ${error.message}; body=${responseBody}`));
            return;
          }
          resolve({ status: response.statusCode, body: parsed });
        });
      }
    );
    request.on('timeout', () => request.destroy(new Error(`${method} ${endpointPath} timed out`)));
    request.on('error', reject);
    if (body) request.write(body);
    request.end();
  });
}

async function tryFirstSuccessful(server, attempts) {
  const observations = [];
  for (const attempt of attempts) {
    try {
      const response = await requestJson(server, attempt.method, attempt.path, attempt.payload);
      observations.push({ method: attempt.method, path: attempt.path, status: response.status, body: response.body });
      if (response.status >= 200 && response.status < 300) {
        return { response, observations, attempt };
      }
    } catch (error) {
      observations.push({ method: attempt.method, path: attempt.path, error: error.message });
    }
  }
  return { response: null, observations, attempt: null };
}

function extractUserAccount(payload, username) {
  if (!payload || typeof payload !== 'object') return null;
  const candidates = [payload, payload.user, payload.account, payload.userAccount];
  for (const candidate of candidates) {
    if (candidate && typeof candidate === 'object' && candidate.username === username) {
      return candidate;
    }
  }
  if (Array.isArray(payload.users)) {
    return payload.users.find((candidate) => candidate && typeof candidate === 'object' && candidate.username === username) || null;
  }
  if (Array.isArray(payload.accounts)) {
    return payload.accounts.find((candidate) => candidate && typeof candidate === 'object' && candidate.username === username) || null;
  }
  return null;
}

async function createOrRetrieveUser(server, username, password, requestedRole) {
  const createAttempts = [
    { method: 'POST', path: '/users', payload: { username, password, role: requestedRole } },
    { method: 'POST', path: '/user-management/users', payload: { username, password, role: requestedRole } },
    { method: 'POST', path: '/accounts', payload: { username, password, role: requestedRole } },
    { method: 'POST', path: '/user-accounts', payload: { username, password, role: requestedRole } },
    { method: 'POST', path: '/auth/register', payload: { username, password, role: requestedRole } }
  ];
  const created = await tryFirstSuccessful(server, createAttempts);
  const createdAccount = created.response && extractUserAccount(created.response.body, username);
  if (createdAccount) return { account: createdAccount, observations: created.observations };

  const retrieveAttempts = [
    { method: 'GET', path: `/users/${encodeURIComponent(username)}` },
    { method: 'GET', path: `/user-management/users/${encodeURIComponent(username)}` },
    { method: 'GET', path: `/accounts/${encodeURIComponent(username)}` },
    { method: 'GET', path: `/user-accounts/${encodeURIComponent(username)}` },
    { method: 'GET', path: '/users' },
    { method: 'GET', path: '/user-management/users' },
    { method: 'GET', path: '/accounts' },
    { method: 'GET', path: '/user-accounts' }
  ];
  const retrieved = await tryFirstSuccessful(server, retrieveAttempts);
  return {
    account: retrieved.response && extractUserAccount(retrieved.response.body, username),
    observations: created.observations.concat(retrieved.observations)
  };
}

async function crit_user_2_0aba36d7() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const username = `jazyk_role_${Date.now()}`;
  const password = 'correct-horse-battery-staple';

  try {
    const result = await createOrRetrieveUser(server, username, password, 'Staff');
    assert.ok(
      result.account,
      `A user account must be retrievable through ${source}; observed ${JSON.stringify(result.observations)}`
    );
    assert.ok(
      Object.prototype.hasOwnProperty.call(result.account, 'role'),
      `Retrieved user account must include a role field; account=${JSON.stringify(result.account)}`
    );
    assert.ok(
      ['Admin', 'Manager', 'Staff'].includes(result.account.role),
      `Retrieved user account role must be one of Admin, Manager, or Staff; account=${JSON.stringify(result.account)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_2_0aba36d7()
    .then(() => {
      console.log('crit_user_2_0aba36d7 passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_2_0aba36d7 };
