import { createContext, useContext, type ReactNode } from 'react';
import { ApiClient } from '../api/client';
import { WorkflowStore } from '../store/WorkflowStore';

export interface NewtonContextValue {
  api: ApiClient;
  store: WorkflowStore;
}

const NewtonContext = createContext<NewtonContextValue | null>(null);

export interface NewtonProviderProps {
  api: ApiClient;
  store?: WorkflowStore;
  children: ReactNode;
}

export function NewtonProvider({ api, store, children }: NewtonProviderProps) {
  const value: NewtonContextValue = { api, store: store ?? new WorkflowStore() };
  return <NewtonContext.Provider value={value}>{children}</NewtonContext.Provider>;
}

export function useNewton(): NewtonContextValue {
  const ctx = useContext(NewtonContext);
  if (!ctx) throw new Error('useNewton must be used within NewtonProvider');
  return ctx;
}
