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

function productStock(product) {
  if (!product || typeof product !== 'object') return undefined;
  if ('stock' in product) return product.stock;
  if ('stockLevel' in product) return product.stockLevel;
  if ('currentStockLevel' in product) return product.currentStockLevel;
  return undefined;
}

function productMatchesId(product, id) {
  return product && typeof product === 'object' && product.id === id;
}

async function crit_inventory_3_ebbaa54f() {
  const root = path.resolve(__dirname, '..');
  const { app, source } = loadBackendApp(root);
  const server = app.listen(0);

  try {
    const product = { id: `stock-bind-${Date.now()}`, name: 'Stock Binding Test Product', stock: 4 };
    const updatedStock = 19;
    const createEndpoints = ['/inventory/products', '/inventory/addProduct', '/products'];
    const updateRequests = [
      { method: 'PUT', path: `/inventory/products/${encodeURIComponent(product.id)}/stock`, body: { stock: updatedStock } },
      { method: 'PATCH', path: `/inventory/products/${encodeURIComponent(product.id)}`, body: { stock: updatedStock } },
      { method: 'POST', path: '/inventory/updateStock', body: { id: product.id, stock: updatedStock } },
      { method: 'POST', path: '/inventory/products/updateStock', body: { id: product.id, stock: updatedStock } }
    ];
    const listEndpoints = ['/inventory/products', '/inventory', '/products'];
    const observations = [];

    for (const createEndpoint of createEndpoints) {
      let createResponse;
      try {
        createResponse = await requestJson(server, 'POST', createEndpoint, product);
      } catch (error) {
        observations.push({ createEndpoint, error: error.message });
        continue;
      }
      observations.push({ createEndpoint, createResponse });
      if (createResponse.status < 200 || createResponse.status >= 300) continue;

      for (const updateRequest of updateRequests) {
        let updateResponse;
        try {
          updateResponse = await requestJson(server, updateRequest.method, updateRequest.path, updateRequest.body);
        } catch (error) {
          observations.push({ createEndpoint, updateRequest, error: error.message });
          continue;
        }
        observations.push({ createEndpoint, updateRequest, updateResponse });
        if (updateResponse.status < 200 || updateResponse.status >= 300) continue;

        for (const listEndpoint of listEndpoints) {
          try {
            const listResponse = await requestJson(server, 'GET', listEndpoint);
            const products = extractProducts(listResponse.body);
            observations.push({ createEndpoint, updateRequest, listEndpoint, listResponse });
            if (listResponse.status >= 200 && listResponse.status < 300 && products) {
              const stored = products.find((candidate) => productMatchesId(candidate, product.id));
              if (stored && productStock(stored) === updatedStock) {
                return;
              }
            }
          } catch (error) {
            observations.push({ createEndpoint, updateRequest, listEndpoint, error: error.message });
          }
        }
      }
    }

    assert.fail(
      `Inventory Management from ${source} must support updateStock so a Warehouse Staff stock update is visible on the Product afterward; observed ${JSON.stringify(observations)}`
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

if (require.main === module) {
  crit_inventory_3_ebbaa54f()
    .then(() => {
      console.log('crit_inventory_3_ebbaa54f passed');
    })
    .catch((error) => {
      console.error(error.stack || error.message);
      process.exit(1);
    });
}

module.exports = { crit_inventory_3_ebbaa54f };
