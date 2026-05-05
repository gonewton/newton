import { Routes, Route, Link } from 'react-router-dom';
import { WorkflowsPage } from './pages/WorkflowsPage';
import { ExecutionsPage } from './pages/ExecutionsPage';
import { MonitorPage } from './pages/MonitorPage';
import { EditorPage } from './pages/EditorPage';
import { PlansPage } from './pages/PlansPage';
import { PortfolioPage } from './pages/PortfolioPage';
import { RequestsPage } from './pages/RequestsPage';
import { ThemePage } from './pages/ThemePage';

export function Router() {
  return (
    <>
      <nav>
        <Link to="/workflows">Workflows</Link>
        <Link to="/executions">Executions</Link>
        <Link to="/monitor">Monitor</Link>
        <Link to="/editor">Editor</Link>
        <Link to="/plans">Plans</Link>
        <Link to="/portfolio">Portfolio</Link>
        <Link to="/requests">Requests</Link>
        <Link to="/theme">Theme</Link>
      </nav>
      <main>
        <Routes>
          <Route path="/" element={<WorkflowsPage />} />
          <Route path="/workflows" element={<WorkflowsPage />} />
          <Route path="/executions" element={<ExecutionsPage />} />
          <Route path="/monitor" element={<MonitorPage />} />
          <Route path="/editor" element={<EditorPage />} />
          <Route path="/plans" element={<PlansPage />} />
          <Route path="/portfolio" element={<PortfolioPage />} />
          <Route path="/requests" element={<RequestsPage />} />
          <Route path="/theme" element={<ThemePage />} />
        </Routes>
      </main>
    </>
  );
}
