import React from 'react';
import { createRoot } from 'react-dom/client';
import { WarehouseApp } from './WarehouseApp';
import './styles.css';

const rootElement = document.getElementById('root');

if (!rootElement) {
  throw new Error('Warehouse interface root element was not found.');
}

createRoot(rootElement).render(
  <React.StrictMode>
    <WarehouseApp />
  </React.StrictMode>,
);
