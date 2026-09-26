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

function requestJson(server, method, endpointPath, payload, extraHeaders = {}) {
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
          ...extraHeaders
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
          resolve({ status: response.statusCode, body: parsed, headers: response.headers });
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

function accountIdFrom(responseBody, fallbackUsername) {
  const candidates = [responseBody, responseBody && responseBody.user, responseBody && responseBody.account, responseBody && responseBody.userAccount, responseBody && responseBody.result];
  for (const candidate of candidates) {
    if (candidate && typeof candidate === 'object') {
      if (candidate.id) return candidate.id;
      if (candidate.userId) return candidate.userId;
    }
  }
  return fallbackUsername;
}

function authHeadersFrom(authResponse) {
  const token = authResponse && authResponse.body && (authResponse.body.token || authResponse.body.accessToken || authResponse.body.sessionToken);
  return token ? { authorization: `Bearer ${token}` } : {};
}

async function postFirstSuccessful(server, paths, payload, extraHeaders = {}) {
  const observations = [];
  for (const endpointPath of paths) {
    try {
      const response = await requestJson(server, 'POST', endpointPath, payload, extraHeaders);
      observations.push({ method: 'POST', path: endpointPath, status: response.status, body: response.body });
      if (success(response)) return { response, endpointPath, observations };
    } catch (error) {
      observations.push({ method: 'POST', path: endpointPath, error: error.message });
    }
  }
  return { response: null, endpointPath: null, observations };
}

async function authenticate(server, username, password) {
  return postFirstSuccessful(server, [
    '/user-management/authenticateUser',
    '/users/authenticateUser',
    '/api/user-management/authenticateUser',
    '/auth/login',
    '/login'
  ], { username, password });
}

async function crit_user_8_2ba679d7() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);
  const suffix = Date.now();
  const username = `jazyk_password_change_${suffix}`;
  const oldPassword = 'old-password-123';
  const newPassword = 'new-password-456';

  try {
    const created = await postFirstSuccessful(server, [
      '/user-management/createUser',
      '/users/createUser',
      '/api/user-management/createUser'
    ], { username, password: oldPassword, role: 'user' });
    assert.ok(created.response, `User Management from ${source} must allow creating or arranging an account before passwordChange; observed ${JSON.stringify(created.observations)}`);

    const beforeChange = await authenticate(server, username, oldPassword);
    assert.ok(beforeChange.response, `Created account must authenticate with the old password before passwordChange; observed ${JSON.stringify(beforeChange.observations)}`);

    const userId = accountIdFrom(created.response.body, username);
    const changed = await postFirstSuccessful(server, [
      '/user-management/passwordChange',
      '/users/passwordChange',
      '/api/user-management/passwordChange'
    ], { id: userId, userId, username, oldPassword, password: oldPassword, newPassword }, authHeadersFrom(beforeChange.response));
    assert.ok(changed.response, `User Management from ${source} must provide passwordChange for the user's own account; observed ${JSON.stringify(changed.observations)}`);

    const oldAfterChange = await authenticate(server, username, oldPassword);
    const newAfterChange = await authenticate(server, username, newPassword);
    assert.ok(
      !oldAfterChange.response && newAfterChange.response,
      `passwordChange must change the password of the user's own account so the old password no longer authenticates and the new password does; old observations ${JSON.stringify(oldAfterChange.observations)}, new observations ${JSON.stringify(newAfterChange.observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_user_8_2ba679d7()
    .then(() => {
      console.log('crit_user_8_2ba679d7 passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_user_8_2ba679d7 };
