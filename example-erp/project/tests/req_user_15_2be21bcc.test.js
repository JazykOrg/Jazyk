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

function managementOperationAttempts() {
  const suffix = Date.now();
  return [
    { method: 'POST', path: '/users', payload: { username: `jazyk_managed_${suffix}`, password: 'secret-123', role: 'user' } },
    { method: 'POST', path: '/user-management/users', payload: { username: `jazyk_managed_${suffix}`, password: 'secret-123', role: 'user' } },
    { method: 'PUT', path: '/users/jazyk-managed-user', payload: { username: `jazyk_managed_${suffix}_updated`, role: 'admin' } },
    { method: 'PATCH', path: '/users/jazyk-managed-user', payload: { username: `jazyk_managed_${suffix}_updated`, role: 'admin' } },
    { method: 'DELETE', path: '/users/jazyk-managed-user' },
    { method: 'POST', path: '/user-management/updateUser', payload: { id: 'jazyk-managed-user', username: `jazyk_managed_${suffix}_updated`, role: 'admin' } },
    { method: 'POST', path: '/user-management/deleteUser', payload: { id: 'jazyk-managed-user' } }
  ];
}

async function observeManagementAttempts(server, headers) {
  const observations = [];
  for (const attempt of managementOperationAttempts()) {
    try {
      const response = await requestJson(server, attempt.method, attempt.path, attempt.payload, headers);
      observations.push({ method: attempt.method, path: attempt.path, status: response.status, body: response.body });
    } catch (error) {
      observations.push({ method: attempt.method, path: attempt.path, error: error.message });
    }
  }
  return observations;
}

function successful(observation) {
  return observation.status >= 200 && observation.status < 300;
}

function denied(observation) {
  return observation.status === 401 || observation.status === 403;
}

function notFound(observation) {
  return observation.status === 404 || observation.status === 405;
}

async function crit_user_15_2c3af900() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);

  try {
    const observations = await observeManagementAttempts(server, { authorization: 'Bearer admin-token', 'x-user-role': 'Admin' });
    assert.ok(
      observations.some(successful),
      `An Admin performing a management operation must be allowed to proceed by ${source}; observed ${JSON.stringify(observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

async function crit_user_16_959e845a() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);

  try {
    const observations = await observeManagementAttempts(server, { authorization: 'Bearer ordinary-user-token', 'x-user-role': 'User' });
    assert.ok(
      !observations.some(successful),
      `A non-Admin performing a management operation must not be allowed to proceed by ${source}; observed ${JSON.stringify(observations)}`
    );
    assert.ok(
      observations.some(denied) || observations.some((observation) => observation.status && !notFound(observation)),
      `At least one management operation must be implemented and denied to a non-Admin by ${source}; observed ${JSON.stringify(observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

const tests = { crit_user_15_2c3af900, crit_user_16_959e845a };

async function main() {
  const selected = process.argv[2];
  if (!selected || !tests[selected]) {
    throw new Error(`Select one test by name: ${Object.keys(tests).join(', ')}`);
  }
  await tests[selected]();
  console.log(`${selected} passed`);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stack || error.message);
    process.exit(1);
  });
}

module.exports = tests;
