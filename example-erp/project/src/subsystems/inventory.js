const initialProducts = [
  { id: 'sku-001', name: 'Sample Warehouse Product', stock: 12 }
];

function cloneProduct(product) {
  return { id: product.id, name: product.name, stock: product.stock };
}

function createInventoryManagement(seedProducts = initialProducts) {
  const products = new Map(seedProducts.map((product) => [product.id, cloneProduct(product)]));

  const subsystem = {
    apiEndpoints: [
      'GET /inventory/products',
      'GET /inventory',
      'POST /inventory/products',
      'POST /inventory/addProduct',
      'PUT /inventory/products/:id/stock',
      'PATCH /inventory/products/:id',
      'POST /inventory/updateStock',
      'POST /inventory/products/updateStock'
    ],
    databaseSchema: {
      products: {
        id: 'text primary key',
        name: 'text not null',
        stock: 'integer not null'
      }
    },
    businessLogic: {}
  };

  subsystem.listProducts = function listProducts() {
    return Array.from(products.values()).map(cloneProduct);
  };

  subsystem.productProperties = ['id', 'name', 'stock'];

  subsystem.addProduct = function addProduct(product) {
    if (!product || typeof product !== 'object') {
      throw Object.assign(new Error('Product payload is required'), { statusCode: 400 });
    }
    const id = String(product.id || '').trim();
    const name = String(product.name || '').trim();
    const stock = Number(product.stock);
    if (!id || !name || !Number.isFinite(stock)) {
      throw Object.assign(new Error('Product id, name, and stock are required'), { statusCode: 400 });
    }
    const stored = { id, name, stock };
    products.set(id, stored);
    return cloneProduct(stored);
  };

  subsystem.updateStock = function updateStock(id, stockValue) {
    const productId = String(id || '').trim();
    const stock = Number(stockValue);
    if (!productId || !Number.isFinite(stock)) {
      throw Object.assign(new Error('Product id and stock are required'), { statusCode: 400 });
    }
    const existing = products.get(productId);
    if (!existing) {
      throw Object.assign(new Error('Product not found'), { statusCode: 404 });
    }
    existing.stock = stock;
    return cloneProduct(existing);
  };

  subsystem.businessLogic = {
    listProducts: subsystem.listProducts,
    addProduct: subsystem.addProduct,
    updateStock: subsystem.updateStock
  };

  return subsystem;
}

module.exports = createInventoryManagement();
module.exports.createInventoryManagement = createInventoryManagement;
