import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { vi, describe, it, expect, beforeEach } from 'vitest';

vi.mock('../lib/tauri-bridge', () => ({
  installPlugin: vi.fn(),
  clearApiKey: vi.fn().mockResolvedValue(undefined),
  listModelsFor: vi.fn().mockResolvedValue([]),
  setApiKeyFor: vi.fn().mockResolvedValue(undefined),
  setActiveModel: vi.fn().mockResolvedValue(undefined),
  setActiveProvider: vi.fn().mockResolvedValue(undefined),
  testApiKeyFor: vi.fn().mockResolvedValue(undefined),
}));

import { installPlugin } from '../lib/tauri-bridge';
import { Settings } from '../components/Settings';

function renderPluginsTab() {
  render(
    <Settings
      mode="panel"
      onSaved={() => {}}
      onClose={() => {}}
      onCleared={() => {}}
    />
  );
  const pluginsTab = screen.getByTestId('settings-tab-plugins');
  fireEvent.click(pluginsTab);
}

describe('Settings plugins tab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (installPlugin as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);
  });

  it('renders plugin install input and button', () => {
    renderPluginsTab();
    expect(screen.getByTestId('plugin-source-input')).toBeInTheDocument();
    expect(screen.getByTestId('plugin-install-btn')).toBeInTheDocument();
  });

  it('install button is disabled when input is empty', () => {
    renderPluginsTab();
    expect(screen.getByTestId('plugin-install-btn')).toBeDisabled();
  });

  it('shows success summary after install', async () => {
    (installPlugin as ReturnType<typeof vi.fn>).mockResolvedValue({
      name: 'test-plugin',
      version: '1.0.0',
      skills_installed: 2,
      agents_installed: 0,
      mcp_keys: [],
      summary: "Installed plugin 'test-plugin' v1.0.0: 2 skill(s), 0 agent(s)",
    });

    renderPluginsTab();

    fireEvent.change(screen.getByTestId('plugin-source-input'), {
      target: { value: 'github:edytlab-community/test-plugin' },
    });
    fireEvent.click(screen.getByTestId('plugin-install-btn'));

    await waitFor(() => {
      expect(screen.getByTestId('plugin-result')).toHaveTextContent('2 skill(s)');
    });
  });

  it('shows error message on failure', async () => {
    (installPlugin as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('network error'));

    renderPluginsTab();

    fireEvent.change(screen.getByTestId('plugin-source-input'), {
      target: { value: 'github:bad/plugin' },
    });
    fireEvent.click(screen.getByTestId('plugin-install-btn'));

    await waitFor(() => {
      expect(screen.getByTestId('plugin-result')).toHaveTextContent('Error:');
    });
  });

  it('calls installPlugin with trimmed source', async () => {
    (installPlugin as ReturnType<typeof vi.fn>).mockResolvedValue({
      name: 'p',
      version: '0.1.0',
      skills_installed: 0,
      agents_installed: 0,
      mcp_keys: [],
      summary: "Installed plugin 'p' v0.1.0: 0 skill(s), 0 agent(s)",
    });

    renderPluginsTab();

    fireEvent.change(screen.getByTestId('plugin-source-input'), {
      target: { value: '  local:/tmp/my-plugin  ' },
    });
    fireEvent.click(screen.getByTestId('plugin-install-btn'));

    await waitFor(() => {
      expect(installPlugin).toHaveBeenCalledWith('local:/tmp/my-plugin');
    });
  });
});
