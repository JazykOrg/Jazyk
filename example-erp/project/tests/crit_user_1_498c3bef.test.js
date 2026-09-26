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
  return candidates.find((candidate) => candidate && typeof candidate === 'object' && candidate.username === username) || null;
}

async function createUser(server, username, password) {
  const createAttempts = [
    { method: 'POST', path: '/users', payload: { username, password, role: 'user' } },
    { method: 'POST', path: '/user-management/users', payload: { username, password, role: 'user' } },
    { method: 'POST', path: '/accounts', payload: { username, password, role: 'user' } },
    { method: 'POST', path: '/user-accounts', payload: { username, password, role: 'user' } },
    { method: 'POST', path: '/auth/register', payload: { username, password, role: 'user' } }
  ];
  const created = await tryFirstSuccessful(server, createAttempts);
  return {
    account: created.response && extractUserAccount(created.response.body, username),
    observations: created.observations
  };
}

async function crit_user_1_498c3bef() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const timestamp = Date.now();
  const firstUsername = `jazyk_unique_id_a_${timestamp}`;
  const secondUsername = `jazyk_unique_id_b_${timestamp}`;
  const password = 'correct-horse-battery-staple';

  try {
    const first = await createUser(server, firstUsername, password);
    const second = await createUser(server, secondUsername, password);

    assert.ok(
      first.account && Object.prototype.hasOwnProperty.call(first.account, 'id'),
      `User account created through ${source} must include an id; observed ${JSON.stringify(first.observations)}`
    );
    assert.ok(
      second.account && Object.prototype.hasOwnProperty.call(second.account, 'id'),
      `Second user account created through ${source} must include an id; observed ${JSON.stringify(second.observations)}`
    );
    assert.notStrictEqual(
      String(first.account.id),
      String(second.account.id),
      `Different users must receive different user account id values; first=${JSON.stringify(first.account)} second=${JSON.stringify(second.account)}`
    );
    assert.strictEqual(
      first.account.username,
      firstUsername,
      `First id must identify only its user, not another account; first=${JSON.stringify(first.account)}`
    );
    assert.strictEqual(
      second.account.username,
      secondUsername,
      `Second id must identify only its user, not another account; second=${JSON.stringify(second.account)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_1_498c3bef()
    .then(() => {
      console.log('crit_user_1_498c3bef passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_1_498c3bef };
