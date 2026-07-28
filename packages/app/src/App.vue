<script setup lang="ts">
import { useAsyncState } from '@vueuse/core'
import { Toaster } from '@/components/ui/sonner'
import { commands } from '@/lib/bindings'

const { state: pong, isLoading } = useAsyncState(async () => {
  const result = await commands.ping()
  return result.status === 'ok' ? result.data : null
}, null)
</script>

<template>
  <div class="flex min-h-screen flex-col">
    <RouterView class="flex-1" />
    <footer class="flex items-center gap-2 border-t px-4 py-1.5 font-mono text-[11px] text-muted-foreground">
      <span
        class="inline-block size-1.5 rounded-full"
        :class="pong ? 'bg-[oklch(0.72_0.11_240)]' : 'bg-muted-foreground/40'"
        aria-hidden="true"
      />
      <span data-testid="core-status">core {{ pong ?? (isLoading ? 'connecting' : 'not connected') }}</span>
    </footer>
    <Toaster />
  </div>
</template>
