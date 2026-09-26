const express = require('express');
const { Pool } = require('pg');
const inventoryManagement = require('./src/subsystems/inventory');

const pool = new Pool({
  connectionString: process.env.DATABASE_URL || 'postgres://warehouse:warehouse@localhost:5432/warehouse_erp'
});

function createApp() {
  const app = express();
  app.use(express.json());

  const listInventory = (_request, response) => {
    response.json({ products: inventoryManagement.listProducts() });
  };

  const addProduct = (request, response) => {
    try {
      const product = inventoryManagement.addProduct(request.body);
      response.status(201).json(product);
    } catch (error) {
      response.status(error.statusCode || 400).json({ error: error.message });
    }
  };

  const updateStockFromBody = (request, response) => {
    try {
      const product = inventoryManagement.updateStock(request.body.id, request.body.stock);
      response.json(product);
    } catch (error) {
      response.status(error.statusCode || 400).json({ error: error.message });
    }
  };

  const updateStockForId = (request, response) => {
    try {
      const product = inventoryManagement.updateStock(request.params.id, request.body.stock);
      response.json(product);
    } catch (error) {
      response.status(error.statusCode || 400).json({ error: error.message });
    }
  };

  app.get('/inventory/products', listInventory);
  app.get('/inventory', listInventory);
  app.get('/products', listInventory);
  app.post('/inventory/products', addProduct);
  app.post('/inventory/addProduct', addProduct);
  app.post('/products', addProduct);
  app.put('/inventory/products/:id/stock', updateStockForId);
  app.patch('/inventory/products/:id', updateStockForId);
  app.post('/inventory/updateStock', updateStockFromBody);
  app.post('/inventory/products/updateStock', updateStockFromBody);

  app.get('/api/frontend/endpoints', (_request, response) => {
    response.json(inventoryManagement.apiEndpoints);
  });

  app.locals.postgresPool = pool;
  return app;
}

const app = createApp();

if (require.main === module) {
  const port = process.env.PORT || 3000;
  app.listen(port, () => {
    console.log(`Warehouse ERP backend listening on ${port}`);
  });
}

module.exports = app;
module.exports.app = app;
module.exports.createApp = createApp;
module.exports.pool = pool;
