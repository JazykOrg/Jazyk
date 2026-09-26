const express = require('express');
const { Pool } = require('pg');
const inventoryManagement = require('./src/subsystems/inventory');
const userManagement = require('./src/subsystems/user-management');

const pool = new Pool({
  connectionString: process.env.DATABASE_URL || 'postgres://warehouse:warehouse@localhost:5432/warehouse_erp'
});

function createBackendApp() {
  const app = express();
  app.use(express.json());

  const subsystems = {
    userManagement,
    inventoryManagement
  };

  app.get('/', (_request, response) => {
    response.json({
      service: 'Warehouse ERP Backend',
      style: 'REST API',
      runtime: 'Node.js',
      framework: 'Express',
      subsystems: Object.keys(subsystems)
    });
  });

  app.get('/subsystems', (_request, response) => {
    response.json({
      subsystems: [
        {
          name: 'User Management',
          key: 'userManagement',
          responsibleFor: 'user accounts and authentication',
          apiEndpoints: userManagement.apiEndpoints,
          databaseSchema: userManagement.databaseSchema,
          businessLogic: Object.keys(userManagement.businessLogic)
        },
        {
          name: 'Inventory Management',
          key: 'inventoryManagement',
          responsibleFor: 'products and stock levels',
          apiEndpoints: inventoryManagement.apiEndpoints,
          databaseSchema: inventoryManagement.databaseSchema,
          businessLogic: Object.keys(inventoryManagement.businessLogic)
        }
      ]
    });
  });

  app.get('/api/frontend/endpoints', (_request, response) => {
    response.json({
      frontend: true,
      endpoints: [
        'GET /',
        'GET /subsystems',
        'GET /api/frontend/endpoints',
        ...userManagement.apiEndpoints,
        ...inventoryManagement.apiEndpoints
      ]
    });
  });

  app.get('/inventory/products', (_request, response) => {
    response.json({ products: inventoryManagement.listProducts() });
  });

  app.post('/inventory/products', (request, response) => {
    try {
      response.status(201).json(inventoryManagement.addProduct(request.body));
    } catch (error) {
      response.status(error.statusCode || 400).json({ error: error.message });
    }
  });

  app.put('/inventory/products/:id/stock', (request, response) => {
    try {
      response.json(inventoryManagement.updateStock(request.params.id, request.body.stock));
    } catch (error) {
      response.status(error.statusCode || 400).json({ error: error.message });
    }
  });

  app.locals.postgresPool = pool;
  app.locals.subsystems = subsystems;
  return app;
}

const app = createBackendApp();

if (require.main === module) {
  const port = process.env.PORT || 3000;
  app.listen(port, () => {
    console.log(`Warehouse ERP backend listening on ${port}`);
  });
}

module.exports = app;
module.exports.app = app;
module.exports.createApp = createBackendApp;
module.exports.createBackendApp = createBackendApp;
module.exports.pool = pool;
