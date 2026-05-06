import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => vi.fn(),
}))

const mockUseApi = vi.fn()
vi.mock('#/hooks/useApi', () => ({
  useApi: (...args: unknown[]) => mockUseApi(...args),
}))

vi.mock('#/api', () => ({
  apiClient: { GET: vi.fn() },
  collectAll: vi.fn(),
}))

import { ProjectSwitcher } from './ProjectSwitcher'

afterEach(() => {
  cleanup()
  mockUseApi.mockReset()
})

describe('ProjectSwitcher', () => {
  it('renders the empty-state message when the API returns no projects', () => {
    mockUseApi.mockReturnValue({
      data: [],
      error: null,
      loading: false,
      reload: () => {},
    })

    render(<ProjectSwitcher currentProjectId={null} />)

    expect(screen.getByText('header.noAccessibleProjects')).toBeTruthy()
  })

  it('renders nothing when the API errors out', () => {
    mockUseApi.mockReturnValue({
      data: null,
      error: new Error('boom'),
      loading: false,
      reload: () => {},
    })

    const { container } = render(<ProjectSwitcher currentProjectId={null} />)
    expect(container.textContent).toBe('')
  })

  it('renders the project name when at least one project is returned', () => {
    mockUseApi.mockReturnValue({
      data: [{ id: 1, name: 'alpha', description: null, created_at: 'x' }],
      error: null,
      loading: false,
      reload: () => {},
    })

    render(<ProjectSwitcher currentProjectId={1} />)

    // The name surfaces both in the trigger and in the dropdown item; both
    // confirm the non-empty render path.
    expect(screen.getAllByText('alpha').length).toBeGreaterThan(0)
    expect(screen.queryByText('header.noAccessibleProjects')).toBeNull()
  })
})
