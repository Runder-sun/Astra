import React from 'react';
import { createRoot } from 'react-dom/client';
import { RemoteApp } from './RemoteApp.jsx';

createRoot(document.getElementById('app')).render(
  <React.StrictMode>
    <RemoteApp />
  </React.StrictMode>,
);
