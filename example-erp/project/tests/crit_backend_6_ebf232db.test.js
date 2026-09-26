const assert = require('assert');
const fs = require('fs');
const path = require('path');

function existsFromRoot(root, relativePath) {
  return fs.existsSync(path.join(root, relativePath));
}

function crit_backend_6_ebf232db() {
  const root = path.resolve(__dirname, '..');

  const inventorySubsystemCandidates = [
    'src/subsystems/inventory',
    'src/subsystems/inventory.js',
    'src/subsystems/inventory.ts',
    'src/subsystems/inventory-management',
    'src/subsystems/inventory-management.js',
    'src/subsystems/inventory-management.ts',
    'backend/src/subsystems/inventory',
    'backend/src/subsystems/inventory.js',
    'backend/src/subsystems/inventory.ts',
    'backend/src/subsystems/inventory-management',
    'backend/src/subsystems/inventory-management.js',
    'backend/src/subsystems/inventory-management.ts',
    'subsystems/inventory',
    'subsystems/inventory.js',
    'subsystems/inventory.ts',
    'subsystems/inventory-management',
    'subsystems/inventory-management.js',
    'subsystems/inventory-management.ts'
  ];

  const present = inventorySubsystemCandidates.filter((candidate) => existsFromRoot(root, candidate));
  assert.ok(
    present.length > 0,
    'backend source must include an Inventory Management subsystem implementation or module under the backend subsystems tree'
  );
}

if (require.main === module) {
  crit_backend_6_ebf232db();
  console.log('crit_backend_6_ebf232db passed');
}

module.exports = { crit_backend_6_ebf232db };
