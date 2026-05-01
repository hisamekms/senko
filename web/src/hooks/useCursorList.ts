import { useCallback, useEffect, useRef, useState } from 'react'

import type { Page } from '#/api'

export interface CursorListState<T> {
  items: T[]
  loading: boolean
  error: Error | null
  hasMore: boolean
  loadMore: () => void
  reload: () => void
}

export function useCursorList<T>(
  fetchPage: (cursor: string | null) => Promise<Page<T>>,
  deps: ReadonlyArray<unknown>,
): CursorListState<T> {
  const [items, setItems] = useState<T[]>([])
  const [cursor, setCursor] = useState<string | null>(null)
  const [hasMore, setHasMore] = useState<boolean>(true)
  const [loading, setLoading] = useState<boolean>(true)
  const [error, setError] = useState<Error | null>(null)
  const [tick, setTick] = useState(0)
  const fetchRef = useRef(fetchPage)
  fetchRef.current = fetchPage

  useEffect(() => {
    let active = true
    setItems([])
    setCursor(null)
    setHasMore(true)
    setError(null)
    setLoading(true)

    fetchRef
      .current(null)
      .then((page) => {
        if (!active) return
        setItems(page.items)
        const next = page.next_cursor ?? null
        setCursor(next)
        setHasMore(next !== null)
        setLoading(false)
      })
      .catch((err: unknown) => {
        if (!active) return
        setError(err instanceof Error ? err : new Error(String(err)))
        setLoading(false)
      })

    return () => {
      active = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick])

  const loadMore = useCallback(() => {
    if (loading || !hasMore || cursor === null) return
    setLoading(true)
    setError(null)
    fetchRef
      .current(cursor)
      .then((page) => {
        setItems((prev) => [...prev, ...page.items])
        const next = page.next_cursor ?? null
        setCursor(next)
        setHasMore(next !== null)
        setLoading(false)
      })
      .catch((err: unknown) => {
        setError(err instanceof Error ? err : new Error(String(err)))
        setLoading(false)
      })
  }, [cursor, hasMore, loading])

  const reload = useCallback(() => {
    setTick((n) => n + 1)
  }, [])

  return { items, loading, error, hasMore, loadMore, reload }
}
