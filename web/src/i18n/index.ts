import i18next from 'i18next'
import LanguageDetector from 'i18next-browser-languagedetector'
import { initReactI18next } from 'react-i18next'

import en from './locales/en.json'
import ja from './locales/ja.json'

export const SUPPORTED_LANGUAGES = ['en', 'ja'] as const
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number]

export const DEFAULT_LANGUAGE: SupportedLanguage = 'en'

const resources = {
  en: { translation: en },
  ja: { translation: ja },
} as const

let initialized = false

export function initI18n() {
  if (initialized) return i18next
  initialized = true

  const isBrowser = typeof window !== 'undefined'
  const instance = i18next

  if (isBrowser) {
    instance.use(LanguageDetector)
  }

  instance.use(initReactI18next).init({
    resources,
    fallbackLng: DEFAULT_LANGUAGE,
    supportedLngs: SUPPORTED_LANGUAGES as unknown as string[],
    lng: isBrowser ? undefined : DEFAULT_LANGUAGE,
    interpolation: { escapeValue: false },
    detection: {
      order: ['localStorage', 'navigator', 'htmlTag'],
      caches: ['localStorage'],
      lookupLocalStorage: 'senko.web.lng',
    },
    react: { useSuspense: false },
  })

  return instance
}

initI18n()

export default i18next
