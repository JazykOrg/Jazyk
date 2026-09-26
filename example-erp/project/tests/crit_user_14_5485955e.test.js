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

async function firstAllowedUsage(server, headers) {
  const attempts = [
    { method: 'GET', path: '/user-management/usage' },
    { method: 'GET', path: '/users/usage' },
    { method: 'GET', path: '/usage' },
    { method: 'POST', path: '/user-management/usage', payload: { operation: 'usage' } },
    { method: 'POST', path: '/users/usage', payload: { operation: 'usage' } },
    { method: 'POST', path: '/usage', payload: { operation: 'usage' } }
  ];
  const observations = [];

  for (const attempt of attempts) {
    try {
      const response = await requestJson(server, attempt.method, attempt.path, attempt.payload, headers);
      observations.push({ method: attempt.method, path: attempt.path, status: response.status, body: response.body });
      if (response.status >= 200 && response.status < 300) {
        return { allowed: true, observations, attempt };
      }
    } catch (error) {
      observations.push({ method: attempt.method, path: attempt.path, error: error.message });
    }
  }

  return { allowed: false, observations, attempt: null };
}

async function crit_user_14_5485955e() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);

  try {
    const representativeUsers = [
      { label: 'anonymous user', headers: {} },
      { label: 'ordinary authenticated user', headers: { authorization: 'Bearer ordinary-user-token' } },
      { label: 'admin authenticated user', headers: { authorization: 'Bearer admin-user-token' } }
    ];

    const failures = [];
    for (const user of representativeUsers) {
      const result = await firstAllowedUsage(server, user.headers);
      if (!result.allowed) {
        failures.push({ user: user.label, observations: result.observations });
      }
    }

    assert.deepStrictEqual(
      failures,
      [],
      `User Management from ${source} must allow every kind of user to proceed with the usage operation; denied or missing usage attempts: ${JSON.stringify(failures)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_14_5485955e()
    .then(() => {
      console.log('crit_user_14_5485955e passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_14_5485955e };
