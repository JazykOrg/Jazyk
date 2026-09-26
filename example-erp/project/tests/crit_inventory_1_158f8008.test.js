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

function requestJson(server, endpointPath) {
  const { port } = server.address();
  return new Promise((resolve, reject) => {
    const request = http.get({ hostname: '127.0.0.1', port, path: endpointPath, timeout: 2000 }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { body += chunk; });
      response.on('end', () => {
        try {
          resolve({ status: response.statusCode, body: body ? JSON.parse(body) : null });
        } catch (error) {
          reject(new Error(`${endpointPath} did not return JSON: ${error.message}; body=${body}`));
        }
      });
    });
    request.on('timeout', () => request.destroy(new Error(`${endpointPath} timed out`)));
    request.on('error', reject);
  });
}

function extractProducts(payload) {
  if (Array.isArray(payload)) return payload;
  if (payload && Array.isArray(payload.products)) return payload.products;
  if (payload && payload.inventory && Array.isArray(payload.inventory.products)) return payload.inventory.products;
  return null;
}

async function crit_inventory_1_158f8008() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);

  try {
    const endpoints = ['/inventory/products', '/inventory', '/products'];
    const responses = [];

    for (const endpoint of endpoints) {
      try {
        const response = await requestJson(server, endpoint);
        responses.push({ endpoint, response });
      } catch (error) {
        responses.push({ endpoint, error: error.message });
      }
    }

    const successful = responses.find(({ response }) => response && response.status >= 200 && response.status < 300 && extractProducts(response.body));
    assert.ok(
      successful,
      `Inventory Management from ${source} must expose managed products as JSON at one of ${endpoints.join(', ')}; observed ${JSON.stringify(responses)}`
    );

    const products = extractProducts(successful.response.body);
    assert.ok(products.length > 0, 'Inventory Management must return at least one managed Product so its stock level can be displayed');
    for (const product of products) {
      assert.ok(product && typeof product === 'object', 'each managed Product must be represented as an object');
      assert.ok('stockLevel' in product || 'stock' in product || 'currentStockLevel' in product, `Product ${JSON.stringify(product)} must include its current stock level`);
    }
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_inventory_1_158f8008()
    .then(() => {
      console.log('crit_inventory_1_158f8008 passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_inventory_1_158f8008 };
