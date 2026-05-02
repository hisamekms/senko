export function assertProductionConfig(): void {
  if (
    process.env.NODE_ENV === 'production' &&
    process.env.WEB_DEV_AUTH_BYPASS === 'true'
  ) {
    throw new Error(
      'Refusing to start: WEB_DEV_AUTH_BYPASS=true is forbidden in production ' +
        '(NODE_ENV=production). Unset WEB_DEV_AUTH_BYPASS or set it to "false".',
    )
  }
}

if (typeof window === 'undefined') {
  assertProductionConfig()
}
