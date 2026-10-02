<script setup lang="ts">
import { useDark } from "@vueuse/core"
import { GLYPHS } from "@/constants/glyphs"

const isDark = useDark()

function toggleTheme(event: MouseEvent) {
  const x = event.clientX
  const y = event.clientY
  const endRadius = Math.hypot(
    Math.max(x, window.innerWidth - x),
    Math.max(y, window.innerHeight - y),
  )

  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches

  if (!document.startViewTransition || reduceMotion) {
    isDark.value = !isDark.value
    return
  }

  const transition = document.startViewTransition(() => {
    isDark.value = !isDark.value
  })

  transition.ready.then(() => {
    document.documentElement.animate(
      {
        clipPath: [
          `circle(0px at ${x}px ${y}px)`,
          `circle(${endRadius}px at ${x}px ${y}px)`,
        ],
      },
      {
        duration: 400,
        easing: "ease-in-out",
        pseudoElement: "::view-transition-new(root)",
      },
    )
  })
}
</script>

<template>
  <el-button :icon="undefined" circle text aria-label="Toggle theme" @click="toggleTheme">
    <span class="glyph" aria-hidden="true">{{ isDark ? GLYPHS.moon : GLYPHS.sun }}</span>
  </el-button>
</template>
