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
            parsed = { rawBody: responseBody, parseError: error.message };
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

function responseCarriesCreatedAccount(body, expectedUsername) {
  if (!body || typeof body !== 'object') return false;
  const candidates = [body, body.user, body.account, body.userAccount, body.result, body.data];
  return candidates.some((candidate) => candidate && typeof candidate === 'object' && (
    candidate.username === expectedUsername ||
    candidate.id ||
    candidate.userId
  ));
}

async function crit_user_9_fccf2268() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const username = `jazyk_create_user_${Date.now()}`;
  const password = 'correct-horse-battery-staple';

  try {
    const created = await tryFirstSuccessful(server, [
      { method: 'POST', path: '/user-management/createUser', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/users/createUser', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/api/user-management/createUser', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/users', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/user-management/users', payload: { username, password, role: 'user' } }
    ]);

    assert.ok(
      created.response,
      `User Management from ${source} must provide a createUser operation that creates a new user account; observed ${JSON.stringify(created.observations)}`
    );
    assert.ok(
      responseCarriesCreatedAccount(created.response.body, username),
      `createUser must return or expose the created user account identity; observed ${JSON.stringify(created.response.body)}`
    );

    const lookup = await tryFirstSuccessful(server, [
      { method: 'GET', path: `/users/${encodeURIComponent(username)}` },
      { method: 'GET', path: `/user-management/users/${encodeURIComponent(username)}` },
      { method: 'GET', path: '/users' },
      { method: 'GET', path: '/user-management/users' }
    ]);

    if (lookup.response) {
      assert.ok(
        JSON.stringify(lookup.response.body).includes(username),
        `Created user account must be observable after createUser; lookup observed ${JSON.stringify(lookup.observations)}`
      );
    }
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_9_fccf2268()
    .then(() => {
      console.log('crit_user_9_fccf2268 passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_9_fccf2268 };
