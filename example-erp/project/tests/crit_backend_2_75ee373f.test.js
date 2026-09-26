const assert = require('assert');
const fs = require('fs');
const path = require('path');

function crit_backend_2_75ee373f() {
  const root = path.resolve(__dirname, '..');
  const packagePath = path.join(root, 'package.json');
  assert.ok(fs.existsSync(packagePath), 'package.json must exist so the backend runtime and framework dependencies can be verified');

  const pkg = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
  const deps = { ...(pkg.dependencies || {}), ...(pkg.devDependencies || {}) };
  assert.ok(deps.express, 'package.json must declare Express as a dependency');

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
  const expressUsers = candidates
    .map((candidate) => path.join(root, candidate))
    .filter((candidate) => fs.existsSync(candidate))
    .filter((candidate) => {
      const body = fs.readFileSync(candidate, 'utf8');
      return /require\(['"]express['"]\)|from ['"]express['"]|import\s+express\s*=\s*require\(['"]express['"]\)/.test(body)
        && /express\s*\(/.test(body);
    });

  assert.ok(expressUsers.length > 0, 'a backend entrypoint must import Express and create an Express application');
}

if (require.main === module) {
  crit_backend_2_75ee373f();
  console.log('crit_backend_2_75ee373f passed');
}

module.exports = { crit_backend_2_75ee373f };
