const assert = require('assert');
const warehouseStaff = require('../src/subsystems/warehouse-staff');

function completeInventoryOperation(actorRole) {
  const headers = {
    'x-user-role': actorRole,
    'x-role': actorRole,
    authorization: `Bearer role:${actorRole}`
  };
  try {
    warehouseStaff.requireWarehouseStaff(headers);
    return { status: 200, body: { allowed: true } };
  } catch (error) {
    return { status: error.statusCode || 500, body: { error: error.message } };
  }
}

async function crit_inventory_4_ae7de1d1() {
  const response = completeInventoryOperation('warehouse-staff');
  assert.ok(
    response.status >= 200 && response.status < 300,
    `Inventory Management must allow warehouse-staff to complete an inventory operation; observed ${JSON.stringify(response)}`
  );
}

async function crit_inventory_5_cba56add() {
  const response = completeInventoryOperation('sales-staff');
  assert.ok(
    response.status === 401 || response.status === 403,
    `Inventory Management must refuse non-warehouse staff inventory operations with 401 or 403; observed ${JSON.stringify(response)}`
  );
}

const tests = { crit_inventory_4_ae7de1d1, crit_inventory_5_cba56add };

if (require.main === module) {
  const selected = process.argv[2];
  const names = selected ? [selected] : Object.keys(tests);
  (async () => {
    for (const name of names) {
      assert.ok(tests[name], `Unknown test ${name}; available tests: ${Object.keys(tests).join(', ')}`);
      await tests[name]();
      console.log(`${name} passed`);
    }
  })().catch((error) => {
    console.error(error.stack || error.message);
    process.exit(1);
  });
}

module.exports = tests;
