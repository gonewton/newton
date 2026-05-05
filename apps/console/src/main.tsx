import React from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { ApiClient, NewtonProvider } from 'newton-ui';
import 'newton-ui/style.css';
import { App } from './App';

const api = new ApiClient({ baseUrl: '/api' });

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <BrowserRouter>
      <NewtonProvider api={api}>
        <App />
      </NewtonProvider>
    </BrowserRouter>
  </React.StrictMode>,
);
