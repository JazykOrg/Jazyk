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

function extractUserId(payload) {
  if (!payload || typeof payload !== 'object') return null;
  const candidates = [payload, payload.user, payload.account, payload.userAccount];
  for (const candidate of candidates) {
    if (candidate && typeof candidate === 'object' && (candidate.id || candidate.userId)) {
      return candidate.id || candidate.userId;
    }
  }
  return null;
}

function payloadStillContainsUser(payload, id, username) {
  if (!payload || typeof payload !== 'object') return false;
  const candidates = [];
  if (Array.isArray(payload)) candidates.push(...payload);
  candidates.push(payload, payload.user, payload.account, payload.userAccount);
  if (Array.isArray(payload.users)) candidates.push(...payload.users);
  if (Array.isArray(payload.accounts)) candidates.push(...payload.accounts);
  return candidates.some((candidate) => candidate && typeof candidate === 'object' && (
    String(candidate.id || candidate.userId || '') === String(id) || candidate.username === username
  ));
}

async function crit_user_11_8c07b26b() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const username = `jazyk_delete_user_${Date.now()}`;
  const password = 'correct-horse-battery-staple';

  try {
    const createAttempts = [
      { method: 'POST', path: '/users', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/user-management/users', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/accounts', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/user-accounts', payload: { username, password, role: 'user' } }
    ];
    const created = await tryFirstSuccessful(server, createAttempts);
    const userId = created.response && extractUserId(created.response.body);
    assert.ok(
      userId,
      `User Management from ${source} must allow an existing user account to be created before deleteUser can delete it; observed ${JSON.stringify(created.observations)}`
    );

    const deleteAttempts = [
      { method: 'DELETE', path: `/users/${userId}` },
      { method: 'DELETE', path: `/user-management/users/${userId}` },
      { method: 'DELETE', path: `/accounts/${userId}` },
      { method: 'DELETE', path: `/user-accounts/${userId}` },
      { method: 'POST', path: `/users/${userId}/deleteUser` },
      { method: 'POST', path: '/user-management/deleteUser', payload: { id: userId } }
    ];
    const deleted = await tryFirstSuccessful(server, deleteAttempts);
    assert.ok(
      deleted.response,
      `An Admin invoking deleteUser on an existing account must observe a successful deletion response; observed ${JSON.stringify(deleted.observations)}`
    );

    const lookupAttempts = [
      { method: 'GET', path: `/users/${userId}` },
      { method: 'GET', path: `/user-management/users/${userId}` },
      { method: 'GET', path: `/accounts/${userId}` },
      { method: 'GET', path: `/user-accounts/${userId}` }
    ];
    const lookedUp = await tryFirstSuccessful(server, lookupAttempts);
    assert.ok(
      !lookedUp.response || !payloadStillContainsUser(lookedUp.response.body, userId, username),
      `deleteUser must remove the user account so it is no longer observable; observed ${JSON.stringify(lookedUp.observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_11_8c07b26b()
    .then(() => {
      console.log('crit_user_11_8c07b26b passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_11_8c07b26b };
