import { describe, expect, it } from 'vitest'

import {
  asBoolean,
  asNumber,
  asOrder,
  asString,
  asStringArray,
} from './searchParams'

describe('searchParams helpers', () => {
  describe('asString', () => {
    it('returns the string when non-empty', () => {
      expect(asString('foo')).toBe('foo')
    })
    it('returns undefined for empty / non-string', () => {
      expect(asString('')).toBeUndefined()
      expect(asString(undefined)).toBeUndefined()
      expect(asString(null)).toBeUndefined()
      expect(asString(42)).toBeUndefined()
    })
  })

  describe('asStringArray', () => {
    it('returns the array filtered to strings', () => {
      expect(asStringArray(['a', 1, 'b'])).toEqual(['a', 'b'])
    })
    it('wraps a single string into a one-element array', () => {
      expect(asStringArray('a')).toEqual(['a'])
    })
    it('returns undefined for empty / unsupported inputs', () => {
      expect(asStringArray([])).toBeUndefined()
      expect(asStringArray('')).toBeUndefined()
      expect(asStringArray(undefined)).toBeUndefined()
      expect(asStringArray(123)).toBeUndefined()
    })
  })

  describe('asBoolean', () => {
    it('returns the boolean primitive verbatim', () => {
      // Three-state filters (e.g. Contracts ?completed) need to round-trip
      // explicit false through validateSearch so the URL state survives a
      // re-validation hop. Two-state filters (e.g. ?ready) wrap the result
      // with `=== true ? true : undefined` at the call site.
      expect(asBoolean(true)).toBe(true)
      expect(asBoolean(false)).toBe(false)
    })
    it('parses "true" / "false" strings', () => {
      expect(asBoolean('true')).toBe(true)
      expect(asBoolean('false')).toBe(false)
    })
    it('returns undefined for unsupported inputs', () => {
      expect(asBoolean(undefined)).toBeUndefined()
      expect(asBoolean('nope')).toBeUndefined()
    })
  })

  describe('asNumber', () => {
    it('returns positive integer for matching string / number', () => {
      expect(asNumber(7)).toBe(7)
      expect(asNumber('42')).toBe(42)
    })
    it('returns undefined for negative / non-integer / unparseable inputs', () => {
      expect(asNumber('-1')).toBeUndefined()
      expect(asNumber('1.5')).toBeUndefined()
      expect(asNumber('abc')).toBeUndefined()
      expect(asNumber('')).toBeUndefined()
      expect(asNumber(undefined)).toBeUndefined()
    })
  })

  describe('asOrder', () => {
    it('only accepts asc / desc', () => {
      expect(asOrder('asc')).toBe('asc')
      expect(asOrder('desc')).toBe('desc')
      expect(asOrder('ASC')).toBeUndefined()
      expect(asOrder('foo')).toBeUndefined()
      expect(asOrder(undefined)).toBeUndefined()
    })
  })
})
