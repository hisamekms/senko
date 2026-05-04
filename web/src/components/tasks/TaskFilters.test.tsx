import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) =>
      opts?.defaultValue ?? key,
  }),
}))

import { TaskFilters, type TaskFilterValues } from './TaskFilters'

afterEach(() => {
  cleanup()
})

describe('TaskFilters', () => {
  describe('priority chips', () => {
    it('adds the clicked priority to an empty selection', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      render(<TaskFilters value={{}} onChange={onChange} />)

      await user.click(screen.getByTestId('task-filter-priority-P1'))

      expect(onChange).toHaveBeenCalledTimes(1)
      expect(onChange).toHaveBeenCalledWith({ priority: ['P1'] })
    })

    it('removes the priority when clicked again (toggle off)', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      const value: TaskFilterValues = { priority: ['P1'] }
      render(<TaskFilters value={value} onChange={onChange} />)

      await user.click(screen.getByTestId('task-filter-priority-P1'))

      expect(onChange).toHaveBeenCalledTimes(1)
      expect(onChange).toHaveBeenCalledWith({ priority: undefined })
    })

    it('appends to an existing selection without dropping prior values', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      const value: TaskFilterValues = { priority: ['P0'] }
      render(<TaskFilters value={value} onChange={onChange} />)

      await user.click(screen.getByTestId('task-filter-priority-P2'))

      expect(onChange).toHaveBeenCalledWith({ priority: ['P0', 'P2'] })
    })
  })

  describe('title input', () => {
    it('emits the trimmed title on blur', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      render(<TaskFilters value={{}} onChange={onChange} />)

      const input = screen.getByTestId('task-filter-title')
      await user.type(input, '  hello  ')
      // Blur by tabbing away from the input.
      await user.tab()

      expect(onChange).toHaveBeenCalled()
      const last = onChange.mock.calls.at(-1)?.[0]
      expect(last).toEqual({ title: 'hello' })
    })

    it('emits the trimmed title when Enter is pressed', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      render(<TaskFilters value={{}} onChange={onChange} />)

      const input = screen.getByTestId('task-filter-title')
      await user.type(input, '  hello  {Enter}')

      expect(onChange).toHaveBeenCalled()
      const last = onChange.mock.calls.at(-1)?.[0]
      expect(last).toEqual({ title: 'hello' })
    })
  })

  describe('reset button', () => {
    it('clears both priority and title in the emitted onChange payload', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      const value: TaskFilterValues = {
        priority: ['P0', 'P1'],
        title: 'foo',
      }
      render(<TaskFilters value={value} onChange={onChange} />)

      await user.click(screen.getByTestId('task-filter-reset'))

      expect(onChange).toHaveBeenCalledTimes(1)
      const payload = onChange.mock.calls[0]?.[0] as TaskFilterValues
      expect(payload.priority).toBeUndefined()
      expect(payload.title).toBeUndefined()
    })
  })
})
