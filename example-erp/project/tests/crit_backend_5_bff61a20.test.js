const assert = require('assert');
const fs = require('fs');
const path = require('path');

function existsFromRoot(root, relativePath) {
  return fs.existsSync(path.join(root, relativePath));
}

function crit_backend_5_bff61a20() {
  const root = path.resolve(__dirname, '..');

  const userSubsystemCandidates = [
    'src/subsystems/user',
    'src/subsystems/user.js',
    'src/subsystems/user.ts',
    'src/subsystems/users',
    'src/subsystems/users.js',
    'src/subsystems/users.ts',
    'backend/src/subsystems/user',
    'backend/src/subsystems/user.js',
    'backend/src/subsystems/user.ts',
    'backend/src/subsystems/users',
    'backend/src/subsystems/users.js',
    'backend/src/subsystems/users.ts',
    'subsystems/user',
    'subsystems/user.js',
    'subsystems/user.ts',
    'subsystems/users',
    'subsystems/users.js',
    'subsystems/users.ts'
  ];

  const present = userSubsystemCandidates.filter((candidate) => existsFromRoot(root, candidate));
  assert.ok(
    present.length > 0,
    'backend source must include a User Management subsystem implementation or module under the backend subsystems tree'
  );
}

if (require.main === module) {
  crit_backend_5_bff61a20();
  console.log('crit_backend_5_bff61a20 passed');
}

module.exports = { crit_backend_5_bff61a20 };
