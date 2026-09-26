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

function resetPasswordObserved(payload, oldPassword, newPassword) {
  if (!payload || typeof payload !== 'object') return false;
  const candidates = [payload, payload.user, payload.account, payload.userAccount];
  return candidates.some((candidate) => {
    if (!candidate || typeof candidate !== 'object') return false;
    const password = candidate.password || candidate.temporaryPassword || candidate.newPassword || candidate.resetPassword;
    const resetFlag = candidate.passwordReset === true || candidate.reset === true || candidate.requiresPasswordChange === true;
    return (password && password !== oldPassword) || resetFlag || (newPassword && password === newPassword);
  }) || payload.passwordReset === true || payload.reset === true;
}

async function crit_user_12_5f03a9b3() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const username = `jazyk_password_reset_${Date.now()}`;
  const oldPassword = 'old-password-123';
  const newPassword = 'new-password-456';

  try {
    const createAttempts = [
      { method: 'POST', path: '/users', payload: { username, password: oldPassword, role: 'user' } },
      { method: 'POST', path: '/user-management/users', payload: { username, password: oldPassword, role: 'user' } },
      { method: 'POST', path: '/accounts', payload: { username, password: oldPassword, role: 'user' } },
      { method: 'POST', path: '/user-accounts', payload: { username, password: oldPassword, role: 'user' } }
    ];
    const created = await tryFirstSuccessful(server, createAttempts);
    const userId = created.response && extractUserId(created.response.body);
    assert.ok(
      userId,
      `User Management from ${source} must allow an existing user account to be created before passwordReset can reset it; observed ${JSON.stringify(created.observations)}`
    );

    const resetAttempts = [
      { method: 'POST', path: `/users/${userId}/passwordReset`, payload: { newPassword } },
      { method: 'POST', path: `/users/${userId}/password-reset`, payload: { newPassword } },
      { method: 'POST', path: `/users/${userId}/reset-password`, payload: { newPassword } },
      { method: 'POST', path: `/user-management/users/${userId}/passwordReset`, payload: { newPassword } },
      { method: 'POST', path: `/user-management/users/${userId}/password-reset`, payload: { newPassword } },
      { method: 'POST', path: '/user-management/passwordReset', payload: { id: userId, newPassword } },
      { method: 'POST', path: '/user-management/password-reset', payload: { id: userId, newPassword } }
    ];
    const reset = await tryFirstSuccessful(server, resetAttempts);
    assert.ok(
      reset.response && resetPasswordObserved(reset.response.body, oldPassword, newPassword),
      `An Admin invoking passwordReset on a user account must observe that the account password is reset; observed ${JSON.stringify(reset.observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_12_5f03a9b3()
    .then(() => {
      console.log('crit_user_12_5f03a9b3 passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_12_5f03a9b3 };
