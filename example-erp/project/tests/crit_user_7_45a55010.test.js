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

async function tryAuthenticateUser(server, username, password) {
  const attempts = [
    { method: 'POST', path: '/user-management/authenticateUser', payload: { username, password } },
    { method: 'POST', path: '/users/authenticateUser', payload: { username, password } },
    { method: 'POST', path: '/api/user-management/authenticateUser', payload: { username, password } },
    { method: 'POST', path: '/auth/login', payload: { username, password } },
    { method: 'POST', path: '/login', payload: { username, password } }
  ];
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

function containsAuthenticationResult(payload, username) {
  if (!payload || typeof payload !== 'object') return false;
  if (payload.authenticated === true || payload.success === true || typeof payload.token === 'string') return true;
  const candidates = [payload, payload.user, payload.account, payload.userAccount, payload.result];
  return candidates.some((candidate) => candidate && typeof candidate === 'object' && candidate.username === username);
}

async function crit_user_7_45a55010() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const username = `jazyk_authenticate_${Date.now()}`;
  const password = 'correct-horse-battery-staple';

  try {
    const authenticated = await tryAuthenticateUser(server, username, password);
    assert.ok(
      authenticated.response && containsAuthenticationResult(authenticated.response.body, username),
      `User Management from ${source} must provide authenticateUser accepting username and password and returning an authentication result for that user; observed ${JSON.stringify(authenticated.observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_7_45a55010()
    .then(() => {
      console.log('crit_user_7_45a55010 passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_7_45a55010 };
