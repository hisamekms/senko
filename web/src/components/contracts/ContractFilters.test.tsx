import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) =>
      opts?.defaultValue ?? key,
  }),
}))

import {
  ContractFilters,
  type ContractFilterValues,
} from './ContractFilters'

afterEach(() => {
  cleanup()
})

describe('ContractFilters', () => {
  describe('completion chips', () => {
    it('emits completed=true when the Completed chip is clicked', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      render(<ContractFilters value={{}} onChange={onChange} />)

      await user.click(screen.getByTestId('contract-filter-completion-completed'))

      expect(onChange).toHaveBeenCalledTimes(1)
      expect(onChange).toHaveBeenCalledWith({ completed: true })
    })

    it('emits completed=false when the In progress chip is clicked', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      const value: ContractFilterValues = { completed: true }
      render(<ContractFilters value={value} onChange={onChange} />)

      await user.click(
        screen.getByTestId('contract-filter-completion-in_progress'),
      )

      expect(onChange).toHaveBeenCalledTimes(1)
      expect(onChange).toHaveBeenCalledWith({ completed: false })
    })

    it('emits completed=undefined when the All chip is clicked', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      const value: ContractFilterValues = { completed: true }
      render(<ContractFilters value={value} onChange={onChange} />)

      await user.click(screen.getByTestId('contract-filter-completion-all'))

      expect(onChange).toHaveBeenCalledTimes(1)
      expect(onChange).toHaveBeenCalledWith({ completed: undefined })
    })
  })

  describe('tag input', () => {
    it('emits the trimmed tag on submit (blur)', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      render(<ContractFilters value={{}} onChange={onChange} />)

      const input = screen.getByTestId('contract-filter-tag')
      await user.type(input, '  backend  ')
      await user.tab()

      expect(onChange).toHaveBeenCalled()
      const last = onChange.mock.calls.at(-1)?.[0]
      expect(last).toEqual({ tag: 'backend' })
    })

    it('emits the trimmed tag on submit (Enter)', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      render(<ContractFilters value={{}} onChange={onChange} />)

      const input = screen.getByTestId('contract-filter-tag')
      await user.type(input, '  backend  {Enter}')

      expect(onChange).toHaveBeenCalled()
      const last = onChange.mock.calls.at(-1)?.[0]
      expect(last).toEqual({ tag: 'backend' })
    })
  })

  describe('title input', () => {
    it('emits the trimmed title on submit (blur)', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      render(<ContractFilters value={{}} onChange={onChange} />)

      const input = screen.getByTestId('contract-filter-title')
      await user.type(input, '  hello  ')
      await user.tab()

      expect(onChange).toHaveBeenCalled()
      const last = onChange.mock.calls.at(-1)?.[0]
      expect(last).toEqual({ title: 'hello' })
    })

    it('emits the trimmed title on submit (Enter)', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      render(<ContractFilters value={{}} onChange={onChange} />)

      const input = screen.getByTestId('contract-filter-title')
      await user.type(input, '  hello  {Enter}')

      expect(onChange).toHaveBeenCalled()
      const last = onChange.mock.calls.at(-1)?.[0]
      expect(last).toEqual({ title: 'hello' })
    })
  })

  describe('reset button', () => {
    it('clears completed, tag, and title in the emitted onChange payload', async () => {
      const user = userEvent.setup()
      const onChange = vi.fn()
      const value: ContractFilterValues = {
        completed: true,
        tag: 'backend',
        title: 'release',
      }
      render(<ContractFilters value={value} onChange={onChange} />)

      await user.click(screen.getByTestId('contract-filter-reset'))

      expect(onChange).toHaveBeenCalledTimes(1)
      const payload = onChange.mock.calls[0]?.[0] as ContractFilterValues
      expect(payload.completed).toBeUndefined()
      expect(payload.tag).toBeUndefined()
      expect(payload.title).toBeUndefined()
    })
  })
})
