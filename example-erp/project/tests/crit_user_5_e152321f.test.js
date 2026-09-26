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

function hasUserAccount(payload, username) {
  if (!payload || typeof payload !== 'object') return false;
  const candidates = [payload, payload.user, payload.account, payload.userAccount];
  return candidates.some((candidate) => candidate && typeof candidate === 'object' && (
    candidate.username === username || candidate.id || candidate.userId
  ));
}

function hasAuthenticationToken(payload) {
  if (!payload || typeof payload !== 'object') return false;
  return Boolean(payload.token || payload.accessToken || payload.sessionToken || payload.sessionId || (payload.auth && (payload.auth.token || payload.auth.sessionId)));
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

async function crit_user_5_e152321f() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const username = `jazyk_user_${Date.now()}`;
  const password = 'correct-horse-battery-staple';

  try {
    const createAttempts = [
      { method: 'POST', path: '/users', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/user-management/users', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/accounts', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/user-accounts', payload: { username, password, role: 'user' } },
      { method: 'POST', path: '/auth/register', payload: { username, password, role: 'user' } }
    ];
    const created = await tryFirstSuccessful(server, createAttempts);
    assert.ok(
      created.response && hasUserAccount(created.response.body, username),
      `User Management from ${source} must let a client manage/create a user account with a username and password; observed ${JSON.stringify(created.observations)}`
    );

    const loginAttempts = [
      { method: 'POST', path: '/auth/login', payload: { username, password } },
      { method: 'POST', path: '/login', payload: { username, password } },
      { method: 'POST', path: '/sessions', payload: { username, password } },
      { method: 'POST', path: '/user-management/authenticate', payload: { username, password } },
      { method: 'POST', path: '/users/authenticate', payload: { username, password } }
    ];
    const authenticated = await tryFirstSuccessful(server, loginAttempts);
    assert.ok(
      authenticated.response && hasAuthenticationToken(authenticated.response.body),
      `User Management from ${source} must authenticate the managed user with username and password and return a session or token; observed ${JSON.stringify(authenticated.observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_5_e152321f()
    .then(() => {
      console.log('crit_user_5_e152321f passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_5_e152321f };
