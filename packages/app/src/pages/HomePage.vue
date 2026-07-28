<script setup lang="ts">
import { useAsyncState } from '@vueuse/core'
import { commands } from '@/lib/bindings'

const { state: pong, isLoading } = useAsyncState(async () => {
  const result = await commands.ping()
  return result.status === 'ok' ? result.data : null
}, null)
</script>

<template>
  <main class="flex min-h-screen flex-col items-center justify-center">
    <div class="space-y-5">
      <p class="font-mono text-4xl tracking-tight">
        <span class="font-semibold text-foreground">soquel</span><span class="text-muted-foreground">=#</span>
        <span class="prompt-cursor ml-2 inline-block h-7 w-3.5 translate-y-0.5 bg-[oklch(0.72_0.11_240)]" aria-hidden="true" />
      </p>
      <p class="text-sm text-muted-foreground">
        next generation database client
      </p>
      <p class="flex items-center gap-2 font-mono text-xs text-muted-foreground">
        <span
          class="inline-block size-1.5 rounded-full"
          :class="pong ? 'bg-[oklch(0.72_0.11_240)]' : 'bg-muted-foreground/40'"
        />
        core {{ pong ?? (isLoading ? 'connecting' : 'not connected') }}
      </p>
    </div>
  </main>
</template>

<style scoped>
.prompt-cursor {
  animation: blink 1.2s steps(1) infinite;
}

@keyframes blink {
  50% {
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .prompt-cursor {
    animation: none;
  }
}
</style>
