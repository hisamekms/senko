import type { ChangeEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { SUPPORTED_LANGUAGES, type SupportedLanguage } from '#/i18n'
import { css } from '../../styled-system/css'

const wrapperStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '2',
})

const selectStyle = css({
  paddingX: '2',
  paddingY: '1',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  color: 'fg',
  fontSize: 'sm',
})

export function LanguageSwitcher() {
  const { t, i18n } = useTranslation()

  const handleChange = (event: ChangeEvent<HTMLSelectElement>) => {
    const next = event.target.value as SupportedLanguage
    void i18n.changeLanguage(next)
  }

  const current = (i18n.resolvedLanguage ?? i18n.language ?? 'en') as SupportedLanguage

  return (
    <label className={wrapperStyle}>
      <span>{t('header.language')}:</span>
      <select
        className={selectStyle}
        value={SUPPORTED_LANGUAGES.includes(current) ? current : 'en'}
        onChange={handleChange}
        aria-label={t('header.language')}
      >
        {SUPPORTED_LANGUAGES.map((lng) => (
          <option key={lng} value={lng}>
            {t(`language.${lng}`)}
          </option>
        ))}
      </select>
    </label>
  )
}
