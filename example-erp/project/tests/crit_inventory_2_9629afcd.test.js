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

function requestJson(server, method, endpointPath, body) {
  const { port } = server.address();
  return new Promise((resolve, reject) => {
    const serialized = body === undefined ? null : JSON.stringify(body);
    const request = http.request(
      {
        hostname: '127.0.0.1',
        port,
        path: endpointPath,
        method,
        timeout: 2000,
        headers: serialized
          ? { 'content-type': 'application/json', 'content-length': Buffer.byteLength(serialized) }
          : undefined
      },
      (response) => {
        let responseBody = '';
        response.setEncoding('utf8');
        response.on('data', (chunk) => { responseBody += chunk; });
        response.on('end', () => {
          try {
            resolve({ status: response.statusCode, body: responseBody ? JSON.parse(responseBody) : null });
          } catch (error) {
            reject(new Error(`${method} ${endpointPath} did not return JSON: ${error.message}; body=${responseBody}`));
          }
        });
      }
    );
    request.on('timeout', () => request.destroy(new Error(`${method} ${endpointPath} timed out`)));
    request.on('error', reject);
    if (serialized) request.write(serialized);
    request.end();
  });
}

function extractProducts(payload) {
  if (Array.isArray(payload)) return payload;
  if (payload && Array.isArray(payload.products)) return payload.products;
  if (payload && payload.inventory && Array.isArray(payload.inventory.products)) return payload.inventory.products;
  return null;
}

function productMatches(product, expected) {
  return product && typeof product === 'object'
    && product.id === expected.id
    && product.name === expected.name
    && (product.stock === expected.stock || product.stockLevel === expected.stock || product.currentStockLevel === expected.stock);
}

async function crit_inventory_2_9629afcd() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);

  try {
    const newProduct = { id: `bind-${Date.now()}`, name: 'Binding Test Product', stock: 7 };
    const addEndpoints = ['/inventory/products', '/inventory/addProduct', '/products'];
    const listEndpoints = ['/inventory/products', '/inventory', '/products'];
    const attempts = [];

    for (const addEndpoint of addEndpoints) {
      let addResponse;
      try {
        addResponse = await requestJson(server, 'POST', addEndpoint, newProduct);
      } catch (error) {
        attempts.push({ addEndpoint, error: error.message });
        continue;
      }
      attempts.push({ addEndpoint, addResponse });
      if (addResponse.status < 200 || addResponse.status >= 300) continue;

      for (const listEndpoint of listEndpoints) {
        try {
          const listResponse = await requestJson(server, 'GET', listEndpoint);
          const products = extractProducts(listResponse.body);
          attempts.push({ addEndpoint, listEndpoint, listResponse });
          if (listResponse.status >= 200 && listResponse.status < 300 && products && products.some((product) => productMatches(product, newProduct))) {
            return;
          }
        } catch (error) {
          attempts.push({ addEndpoint, listEndpoint, error: error.message });
        }
      }
    }

    assert.fail(
      `Inventory Management from ${source} must support addProduct by accepting a new Product and returning it in inventory afterward; observed ${JSON.stringify(attempts)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_inventory_2_9629afcd()
    .then(() => {
      console.log('crit_inventory_2_9629afcd passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_inventory_2_9629afcd };
