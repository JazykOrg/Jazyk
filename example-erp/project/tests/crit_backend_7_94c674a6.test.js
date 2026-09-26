const assert = require('assert');
const fs = require('fs');
const path = require('path');

function existingCandidate(root, candidates) {
  return candidates.map((candidate) => path.join(root, candidate)).find((candidatePath) => fs.existsSync(candidatePath));
}

function loadSubsystem(root, name, candidates) {
  const found = existingCandidate(root, candidates);
  assert.ok(found, `backend must include a ${name} subsystem module`);
  // Requiring the module executes the deliverable code rather than reading a prose description.
  // The generated module may export either the subsystem object directly or as a default export.
  // eslint-disable-next-line global-require, import/no-dynamic-require
  const loaded = require(found);
  return loaded && loaded.default ? loaded.default : loaded;
}

function hasEndpoints(subsystem) {
  return Array.isArray(subsystem.apiEndpoints) && subsystem.apiEndpoints.length > 0;
}

function hasDatabaseSchema(subsystem) {
  return subsystem.databaseSchema && typeof subsystem.databaseSchema === 'object' && Object.keys(subsystem.databaseSchema).length > 0;
}

function hasBusinessLogic(subsystem) {
  if (typeof subsystem.businessLogic === 'function') {
    return true;
  }
  return subsystem.businessLogic && typeof subsystem.businessLogic === 'object' && Object.keys(subsystem.businessLogic).length > 0;
}

function assertSubsystemShape(name, subsystem) {
  assert.ok(subsystem && typeof subsystem === 'object', `${name} subsystem must export an object`);
  assert.ok(hasEndpoints(subsystem), `${name} subsystem must define API endpoints`);
  assert.ok(hasDatabaseSchema(subsystem), `${name} subsystem must define a database schema`);
  assert.ok(hasBusinessLogic(subsystem), `${name} subsystem must define business logic`);
}

function crit_backend_7_94c674a6() {
  const root = path.resolve(__dirname, '..');

  const userManagement = loadSubsystem(root, 'User Management', [
    'src/subsystems/user-management.js',
    'src/subsystems/user-management/index.js',
    'src/subsystems/users.js',
    'src/subsystems/users/index.js',
    'backend/src/subsystems/user-management.js',
    'backend/src/subsystems/user-management/index.js',
    'backend/src/subsystems/users.js',
    'backend/src/subsystems/users/index.js',
    'subsystems/user-management.js',
    'subsystems/user-management/index.js',
    'subsystems/users.js',
    'subsystems/users/index.js'
  ]);

  const inventoryManagement = loadSubsystem(root, 'Inventory Management', [
    'src/subsystems/inventory-management.js',
    'src/subsystems/inventory-management/index.js',
    'src/subsystems/inventory.js',
    'src/subsystems/inventory/index.js',
    'backend/src/subsystems/inventory-management.js',
    'backend/src/subsystems/inventory-management/index.js',
    'backend/src/subsystems/inventory.js',
    'backend/src/subsystems/inventory/index.js',
    'subsystems/inventory-management.js',
    'subsystems/inventory-management/index.js',
    'subsystems/inventory.js',
    'subsystems/inventory/index.js'
  ]);

  assertSubsystemShape('User Management', userManagement);
  assertSubsystemShape('Inventory Management', inventoryManagement);
}

if (require.main === module) {
  crit_backend_7_94c674a6();
  console.log('crit_backend_7_94c674a6 passed');
}

module.exports = { crit_backend_7_94c674a6 };
