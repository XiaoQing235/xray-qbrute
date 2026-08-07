import { icons } from '@m234/nerd-fonts/icons'

function glyph(name: string): string {
  const entry = (icons as Record<string, { value: string }>)[name]
  if (!entry) {
    if (import.meta.env.DEV) {
      console.warn(`[glyphs] NF icon "${name}" not found in @m234/nerd-fonts`)
    }
    return ''
  }
  return entry.value
}

export const GLYPHS = {
  play: glyph('nf-fa-play'),
  stop: glyph('nf-fa-stop'),
  sun: glyph('nf-fa-sun'),
  moon: glyph('nf-fa-moon'),
  copy: glyph('nf-fa-copy'),
  check: glyph('nf-fa-check'),
  chevronDown: glyph('nf-fa-chevron_down'),
  circle: glyph('nf-fa-circle_o'),
  spinner: glyph('nf-fa-spinner'),
  flag: glyph('nf-fa-flag_checkered'),
  square: glyph('nf-fa-stop'),
  ban: glyph('nf-fa-ban'),
  alertTriangle: glyph('nf-fa-exclamation_triangle'),
  github: glyph('nf-fa-github'),
} as const
