const assert = require('assert');
const fs = require('fs');
const http = require('http');
const path = require('path');
const { spawn } = require('child_process');

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function request(port, pathname) {
  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        hostname: '127.0.0.1',
        port,
        path: pathname,
        method: 'GET',
        timeout: 1000,
        headers: { Accept: 'application/json' }
      },
      (res) => {
        let body = '';
        res.setEncoding('utf8');
        res.on('data', (chunk) => { body += chunk; });
        res.on('end', () => resolve({ statusCode: res.statusCode, headers: res.headers, body }));
      }
    );
    req.on('timeout', () => req.destroy(new Error('request timed out')));
    req.on('error', reject);
    req.end();
  });
}

async function waitForHttp(port) {
  let lastError;
  for (let i = 0; i < 25; i += 1) {
    try {
      return await request(port, '/');
    } catch (error) {
      lastError = error;
      await wait(200);
    }
  }
  throw lastError || new Error('backend did not accept HTTP requests');
}

async function crit_main_1_892c8f8b() {
  const root = path.resolve(__dirname, '..');
  const packagePath = path.join(root, 'package.json');
  assert.ok(fs.existsSync(packagePath), 'package.json must exist so a Warehouse consumer can start the Backend REST API');

  const pkg = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
  assert.ok(pkg.scripts && pkg.scripts.start, 'package.json must provide an npm start script for the Backend REST API');

  const port = 40104;
  const child = spawn(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['start'], {
    cwd: root,
    env: { ...process.env, PORT: String(port), NODE_ENV: 'test' },
    stdio: ['ignore', 'pipe', 'pipe']
  });

  let output = '';
  child.stdout.on('data', (chunk) => { output += chunk.toString(); });
  child.stderr.on('data', (chunk) => { output += chunk.toString(); });

  try {
    const response = await waitForHttp(port);
    assert.notStrictEqual(response.statusCode, 404, 'Backend REST API must expose at least one HTTP resource to Warehouse consumers');
    const contentType = String(response.headers['content-type'] || '');
    assert.ok(
      contentType.includes('application/json') || response.body.trim().startsWith('{') || response.body.trim().startsWith('['),
      'Backend REST API root resource should be consumable as JSON'
    );
  } finally {
    child.kill('SIGTERM');
    await wait(100);
    if (!child.killed) child.kill('SIGKILL');
  }

  assert.notStrictEqual(child.exitCode, 1, `Backend REST API process exited before serving HTTP requests: ${output}`);
}

if (require.main === module) {
  crit_main_1_892c8f8b()
    .then(() => console.log('crit_main_1_892c8f8b passed'))
    .catch((error) => {
      console.error(error && error.stack ? error.stack : error);
      process.exit(1);
    });
}

module.exports = { crit_main_1_892c8f8b };
