import { useCallback, useEffect, useState } from 'react'

export type Theme = 'light' | 'dark'

const STORAGE_KEY = 'senko.web.theme'

function readStoredTheme(): Theme | null {
  if (typeof window === 'undefined') return null
  const v = window.localStorage.getItem(STORAGE_KEY)
  return v === 'light' || v === 'dark' ? v : null
}

function readSystemTheme(): Theme {
  if (typeof window === 'undefined') return 'light'
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function applyTheme(theme: Theme) {
  if (typeof document === 'undefined') return
  document.documentElement.dataset.theme = theme
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>('light')
  const [hydrated, setHydrated] = useState(false)
  const [followSystem, setFollowSystem] = useState(true)

  useEffect(() => {
    const stored = readStoredTheme()
    if (stored) {
      setThemeState(stored)
      setFollowSystem(false)
      applyTheme(stored)
    } else {
      const sys = readSystemTheme()
      setThemeState(sys)
      setFollowSystem(true)
      applyTheme(sys)
    }
    setHydrated(true)
  }, [])

  useEffect(() => {
    if (!followSystem || typeof window === 'undefined') return
    const mql = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = (e: MediaQueryListEvent) => {
      const next: Theme = e.matches ? 'dark' : 'light'
      setThemeState(next)
      applyTheme(next)
    }
    mql.addEventListener('change', handler)
    return () => mql.removeEventListener('change', handler)
  }, [followSystem])

  const setTheme = useCallback((next: Theme) => {
    setThemeState(next)
    setFollowSystem(false)
    applyTheme(next)
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(STORAGE_KEY, next)
    }
  }, [])

  const toggleTheme = useCallback(() => {
    setTheme(theme === 'dark' ? 'light' : 'dark')
  }, [theme, setTheme])

  return { theme, setTheme, toggleTheme, followSystem, hydrated }
}
