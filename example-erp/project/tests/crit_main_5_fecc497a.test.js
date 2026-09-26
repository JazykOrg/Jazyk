const assert = require('assert');
const fs = require('fs');
const path = require('path');

function crit_main_5_fecc497a() {
  const root = path.resolve(__dirname, '..');
  const candidateEntrypoints = [
    'package.json',
    'server.js',
    'src/server.js',
    'backend/server.js',
    'backend/src/server.js',
    'app.js',
    'src/app.js',
    'backend/app.js',
    'backend/src/app.js',
    'client/src/App.tsx',
    'client/src/App.jsx',
    'src/App.tsx',
    'src/App.jsx'
  ];

  const existing = candidateEntrypoints.filter((candidate) => fs.existsSync(path.join(root, candidate)));
  assert.ok(existing.length > 0, 'Warehouse ERP application source must exist');

  const combined = existing.map((candidate) => fs.readFileSync(path.join(root, candidate), 'utf8')).join('\n');
  assert.ok(/warehouse management system/i.test(combined), 'application must present itself as a warehouse management system');
  assert.ok(/ACME warehouse/i.test(combined), 'application must identify the managed warehouse as the ACME warehouse');
}

if (require.main === module) {
  crit_main_5_fecc497a();
  console.log('crit_main_5_fecc497a passed');
}

module.exports = { crit_main_5_fecc497a };
