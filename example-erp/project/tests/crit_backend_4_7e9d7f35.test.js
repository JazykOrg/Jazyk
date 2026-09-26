const assert = require('assert');
const fs = require('fs');
const path = require('path');

function readIfExists(filePath) {
  return fs.existsSync(filePath) ? fs.readFileSync(filePath, 'utf8') : '';
}

function crit_backend_4_7e9d7f35() {
  const root = path.resolve(__dirname, '..');
  const packagePath = path.join(root, 'package.json');
  assert.ok(fs.existsSync(packagePath), 'package.json must exist so PostgreSQL client dependencies can be verified');

  const pkg = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
  const deps = { ...(pkg.dependencies || {}), ...(pkg.devDependencies || {}) };
  assert.ok(deps.pg || deps.postgres || deps['sequelize'] || deps['typeorm'] || deps['knex'], 'backend must declare a PostgreSQL-capable database dependency');

  const candidates = [
    'server.js',
    'src/server.js',
    'backend/server.js',
    'backend/src/server.js',
    'app.js',
    'src/app.js',
    'backend/app.js',
    'backend/src/app.js',
    'db.js',
    'src/db.js',
    'backend/db.js',
    'backend/src/db.js',
    'database.js',
    'src/database.js',
    'backend/database.js',
    'backend/src/database.js'
  ];

  const databaseUsers = candidates
    .map((candidate) => path.join(root, candidate))
    .filter((candidate) => fs.existsSync(candidate))
    .filter((candidate) => {
      const body = readIfExists(candidate);
      return /require\(['"]pg['"]\)|from ['"]pg['"]|new\s+Pool\s*\(|new\s+Client\s*\(|postgres(?:ql)?:\/\//i.test(body);
    });

  assert.ok(databaseUsers.length > 0, 'backend runtime source must configure or open a PostgreSQL connection for warehouse operations data');
}

if (require.main === module) {
  crit_backend_4_7e9d7f35();
  console.log('crit_backend_4_7e9d7f35 passed');
}

module.exports = { crit_backend_4_7e9d7f35 };
