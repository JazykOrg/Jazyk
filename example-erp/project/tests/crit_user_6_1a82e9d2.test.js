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

function success(response) {
  return response.status >= 200 && response.status < 300;
}

function operationAttempts(operation, suffix) {
  const userId = `jazyk-user-${suffix}`;
  const username = `jazyk_${operation}_${suffix}`;
  const payloads = {
    authenticateUser: { username, password: 'secret-123' },
    passwordChange: { id: userId, oldPassword: 'secret-123', newPassword: 'secret-456' },
    createUser: { username, password: 'secret-123', role: 'user' },
    updateUser: { id: userId, username: `${username}_updated`, role: 'admin' },
    deleteUser: { id: userId },
    passwordReset: { id: userId, newPassword: 'reset-123' }
  };
  return [
    { method: 'POST', path: `/user-management/${operation}`, payload: payloads[operation] },
    { method: 'POST', path: `/users/${operation}`, payload: payloads[operation] },
    { method: 'POST', path: `/api/user-management/${operation}`, payload: payloads[operation] }
  ];
}

async function supportsOperation(server, operation, suffix) {
  const observations = [];
  for (const attempt of operationAttempts(operation, suffix)) {
    try {
      const response = await requestJson(server, attempt.method, attempt.path, attempt.payload);
      observations.push({ method: attempt.method, path: attempt.path, status: response.status, body: response.body });
      if (success(response)) return { supported: true, observations };
    } catch (error) {
      observations.push({ method: attempt.method, path: attempt.path, error: error.message });
    }
  }
  return { supported: false, observations };
}

async function crit_user_6_1a82e9d2() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const suffix = Date.now();
  const operations = ['authenticateUser', 'passwordChange', 'createUser', 'updateUser', 'deleteUser', 'passwordReset'];
  const results = {};

  try {
    for (const operation of operations) {
      results[operation] = await supportsOperation(server, operation, suffix);
    }
    const missing = operations.filter((operation) => !results[operation].supported);
    assert.deepStrictEqual(
      missing,
      [],
      `User Management from ${source} must support authenticateUser, passwordChange, createUser, updateUser, deleteUser, and passwordReset operations; observed ${JSON.stringify(results)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_6_1a82e9d2()
    .then(() => {
      console.log('crit_user_6_1a82e9d2 passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_6_1a82e9d2 };
