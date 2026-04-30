import { defineConfig } from '@pandacss/dev'

export default defineConfig({
  preflight: true,
  include: ['./src/**/*.{ts,tsx,js,jsx}'],
  exclude: [],
  jsxFramework: 'react',
  outdir: 'styled-system',
  conditions: {
    extend: {
      dark: '[data-theme="dark"] &',
      light: '[data-theme="light"] &',
    },
  },
  theme: {
    extend: {
      tokens: {
        colors: {
          light: {
            bg: { value: '#ffffff' },
            fg: { value: '#1f2328' },
            accent: { value: '#0969da' },
            border: { value: '#d1d9e0' },
            surface: { value: '#f6f8fa' },
          },
          dark: {
            bg: { value: '#0d1117' },
            fg: { value: '#e6edf3' },
            accent: { value: '#58a6ff' },
            border: { value: '#30363d' },
            surface: { value: '#161b22' },
          },
        },
      },
      semanticTokens: {
        colors: {
          bg: { value: { base: '{colors.light.bg}', _dark: '{colors.dark.bg}' } },
          fg: { value: { base: '{colors.light.fg}', _dark: '{colors.dark.fg}' } },
          accent: { value: { base: '{colors.light.accent}', _dark: '{colors.dark.accent}' } },
          border: { value: { base: '{colors.light.border}', _dark: '{colors.dark.border}' } },
          surface: { value: { base: '{colors.light.surface}', _dark: '{colors.dark.surface}' } },
        },
      },
    },
  },
})
