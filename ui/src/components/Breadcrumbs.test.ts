import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import Breadcrumbs from './Breadcrumbs.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';
import { dir } from '../lib/testing';

vi.mock('../lib/api');

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
  vi.mocked(api.getAncestors).mockResolvedValue([dir(0, '/r', 10), dir(2, 'sub', 5)]);
  vi.mocked(api.getNodes).mockResolvedValue([dir(4, 'deep', 1)]);
});

describe('Breadcrumbs', () => {
  test('lists ancestors as buttons and the current folder as text', async () => {
    render(Breadcrumbs, { props: { dirId: 4, rootId: 0, onNavigate: vi.fn() } });
    expect(await screen.findByRole('button', { name: '/r' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'sub' })).toBeInTheDocument();
    expect(screen.getByText('deep')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'deep' })).not.toBeInTheDocument();
    expect(screen.getByRole('navigation', { name: 'Current folder' })).toBeInTheDocument();
    expect(api.getAncestors).toHaveBeenCalledWith(4);
  });

  test('clicking an ancestor navigates to it', async () => {
    const onNavigate = vi.fn();
    render(Breadcrumbs, { props: { dirId: 4, rootId: 0, onNavigate } });
    await userEvent.click(await screen.findByRole('button', { name: 'sub' }));
    expect(onNavigate).toHaveBeenCalledWith(2);
  });

  test('a back button returns to the scanned folder from anywhere below it', async () => {
    const onNavigate = vi.fn();
    render(Breadcrumbs, { props: { dirId: 4, rootId: 0, onNavigate } });
    await userEvent.click(screen.getByRole('button', { name: 'Back to the scanned folder' }));
    expect(onNavigate).toHaveBeenCalledWith(0);
  });

  test('there is no back button while the scanned folder itself is shown', async () => {
    vi.mocked(api.getAncestors).mockResolvedValue([]);
    vi.mocked(api.getNodes).mockResolvedValue([dir(0, '/r', 10)]);
    render(Breadcrumbs, { props: { dirId: 0, rootId: 0, onNavigate: vi.fn() } });
    expect(await screen.findByText('/r')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Back to the scanned folder' })).not.toBeInTheDocument();
  });
});
