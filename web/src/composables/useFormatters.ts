const integerFormatter = new Intl.NumberFormat()

export function formatInteger(value: bigint | number): string {
  return integerFormatter.format(value)
}

export function formatElapsed(milliseconds: number): string {
  return `${(milliseconds / 1000).toFixed(2)} s`
}

export function formatRate(count: bigint | number, milliseconds: number): string {
  if (milliseconds <= 0) return '-'
  const n = typeof count === 'bigint' ? Number(count) : count
  return `${(n / milliseconds / 1000).toFixed(1)} M/s`
}
