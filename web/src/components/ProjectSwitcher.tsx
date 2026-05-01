import { Select, createListCollection } from '@ark-ui/react/select'
import { Portal } from '@ark-ui/react/portal'
import { useNavigate } from '@tanstack/react-router'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '#/hooks/useApi'
import { apiClient, collectAll, type components } from '#/api'
import { css } from '../../styled-system/css'

type Project = components['schemas']['ProjectResponse']

const wrapperStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '2',
  fontSize: 'sm',
})

const labelStyle = css({
  color: 'fg',
})

const triggerStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '2',
  paddingX: '3',
  paddingY: '1.5',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  color: 'fg',
  cursor: 'pointer',
  fontSize: 'sm',
  minWidth: '180px',
  textAlign: 'left',
  justifyContent: 'space-between',
  _hover: { borderColor: 'accent' },
  _disabled: { opacity: '0.6', cursor: 'not-allowed' },
})

const contentStyle = css({
  marginTop: '1',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'bg',
  boxShadow: '0 4px 12px rgba(0, 0, 0, 0.15)',
  paddingY: '1',
  minWidth: '200px',
  zIndex: '100',
})

const itemStyle = css({
  paddingX: '3',
  paddingY: '2',
  fontSize: 'sm',
  color: 'fg',
  cursor: 'pointer',
  _hover: { backgroundColor: 'surface' },
  _highlighted: { backgroundColor: 'surface' },
  '&[data-state="checked"]': {
    fontWeight: 'semibold',
    color: 'accent',
  },
})

const placeholderStyle = css({
  color: 'fg',
  opacity: '0.7',
})

interface ProjectSwitcherProps {
  currentProjectId: number | null
}

export function ProjectSwitcher({ currentProjectId }: ProjectSwitcherProps) {
  const { t } = useTranslation()
  const navigate = useNavigate()

  const { data, loading, error } = useApi<Project[]>(
    () =>
      collectAll<Project>(async (cursor) => {
        const { data, error } = await apiClient.GET('/api/v1/projects', {
          params: { query: cursor ? { after: cursor } : {} },
        })
        if (error || !data) throw new Error('Failed to load projects')
        return { items: data.items, next_cursor: data.next_cursor ?? null }
      }),
    [],
  )

  const projects = data ?? []
  const collection = useMemo(
    () =>
      createListCollection({
        items: projects.map((p) => ({ label: p.name, value: String(p.id) })),
      }),
    [projects],
  )

  const value = currentProjectId !== null ? [String(currentProjectId)] : []

  if (loading) {
    return (
      <span className={wrapperStyle}>
        <span className={labelStyle}>{t('header.project')}:</span>
        <span className={placeholderStyle}>{t('dashboard.loading')}</span>
      </span>
    )
  }

  if (error || projects.length === 0) {
    return null
  }

  return (
    <Select.Root
      collection={collection}
      value={value}
      onValueChange={(details) => {
        const next = details.value[0]
        if (!next) return
        const nextId = Number(next)
        if (Number.isFinite(nextId) && nextId !== currentProjectId) {
          void navigate({
            to: '/p/$projectId',
            params: { projectId: String(nextId) },
          })
        }
      }}
      positioning={{ sameWidth: true }}
    >
      <span className={wrapperStyle}>
        <Select.Label className={labelStyle}>
          {t('header.project')}:
        </Select.Label>
        <Select.Control>
          <Select.Trigger className={triggerStyle} aria-label={t('header.project')}>
            <Select.ValueText placeholder={t('header.projectPlaceholder')} />
            <Select.Indicator>▾</Select.Indicator>
          </Select.Trigger>
        </Select.Control>
      </span>
      <Portal>
        <Select.Positioner>
          <Select.Content className={contentStyle}>
            {projects.map((project) => (
              <Select.Item
                key={project.id}
                item={{ label: project.name, value: String(project.id) }}
                className={itemStyle}
              >
                <Select.ItemText>{project.name}</Select.ItemText>
              </Select.Item>
            ))}
          </Select.Content>
        </Select.Positioner>
      </Portal>
      <Select.HiddenSelect />
    </Select.Root>
  )
}
