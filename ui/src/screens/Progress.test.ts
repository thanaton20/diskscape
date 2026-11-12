import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import Progress from './Progress.svelte';
import { setLocale } from '../lib/i18n';

beforeEach(() => setLocale('en'));

describe('Progress', () => {
  test('shows items, bytes, path, elapsed time and a cancel button', async () => {
    const onCancel = vi.fn();
    render(Progress, {
      props: {
        progress: { items: 1234567, bytes: 1610612736, current: '/Users/me/Library/Caches' },
        startedAt: Date.now() - 12_300,
        onCancel,
      },
    });
    expect(screen.getByText('1,234,567')).toBeInTheDocument();
    expect(screen.getByText('1.5 GB')).toBeInTheDocument();
    expect(screen.getByText('/Users/me/Library/Caches')).toBeInTheDocument();
    expect(screen.getByTestId('elapsed').textContent).toMatch(/^12\.\d s$/);
    await userEvent.click(screen.getByRole('button', { name: /cancel/i }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  test('renders before the first progress event arrives', () => {
    render(Progress, { props: { progress: null, startedAt: Date.now(), onCancel: vi.fn() } });
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
    expect(screen.getByTestId('items').textContent).toBe('0');
  });
});
