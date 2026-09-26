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

function requestJson(server, method, endpointPath, payload, headers = {}) {
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
          'content-length': Buffer.byteLength(body),
          ...headers
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

async function crit_user_13_b32d3a18() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const username = `jazyk_login_${Date.now()}`;
  const password = 'login-password-123';

  try {
    const loginAttempts = [
      { method: 'POST', path: '/login', payload: { username, password } },
      { method: 'POST', path: '/auth/login', payload: { username, password } },
      { method: 'POST', path: '/user-management/login', payload: { username, password } },
      { method: 'POST', path: '/users/login', payload: { username, password } }
    ];

    const observations = [];
    for (const attempt of loginAttempts) {
      try {
        const response = await requestJson(server, attempt.method, attempt.path, attempt.payload);
        observations.push({ method: attempt.method, path: attempt.path, status: response.status, body: response.body });
        if (response.status !== 401 && response.status !== 403) {
          return;
        }
      } catch (error) {
        observations.push({ method: attempt.method, path: attempt.path, error: error.message });
      }
    }

    assert.fail(
      `An unauthenticated user must be allowed to proceed when performing login; ${source} rejected or lacked all unauthenticated login attempts: ${JSON.stringify(observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_13_b32d3a18()
    .then(() => {
      console.log('crit_user_13_b32d3a18 passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_13_b32d3a18 };
