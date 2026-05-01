import { useEffect, useState } from 'react'

export interface ApiState<T> {
  data: T | null
  error: Error | null
  loading: boolean
  reload: () => void
}

export function useApi<T>(
  fetcher: () => Promise<T>,
  deps: ReadonlyArray<unknown>,
): ApiState<T> {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<Error | null>(null)
  const [loading, setLoading] = useState(true)
  const [tick, setTick] = useState(0)

  useEffect(() => {
    let active = true
    setLoading(true)
    setError(null)

    fetcher()
      .then((value) => {
        if (!active) return
        setData(value)
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

  return { data, error, loading, reload: () => setTick((n) => n + 1) }
}
