<script setup lang="ts">
import { Moon, Sun } from '@lucide/vue'
import { useAsyncState } from '@vueuse/core'
import { TooltipProvider } from 'reka-ui'
import { ref } from 'vue'
import CommandPalette from '@/components/CommandPalette.vue'
import { Toaster } from '@/components/ui/sonner'
import { useTheme } from '@/composables/useTheme'
import { commands } from '@/lib/bindings'

const { mode, toggle } = useTheme()

const palette = ref<InstanceType<typeof CommandPalette> | null>(null)

const { state: pong, isLoading } = useAsyncState(async () => {
  const result = await commands.ping()
  return result.status === 'ok' ? result.data : null
}, null)
</script>

<template>
  <TooltipProvider :delay-duration="300">
    <div class="flex h-screen flex-col">
      <div class="min-h-0 flex-1">
        <RouterView />
      </div>
      <footer class="flex items-center gap-2 border-t px-4 py-1 font-mono text-[11px] text-muted-foreground">
        <span
          class="inline-block size-1.5 rounded-full"
          :class="pong ? 'bg-[oklch(0.72_0.11_240)]' : 'bg-muted-foreground/40'"
          aria-hidden="true"
        />
        <span data-testid="core-status">core {{ pong ?? (isLoading ? 'connecting' : 'not connected') }}</span>
        <span class="flex-1" />
        <button
          type="button"
          class="rounded px-1.5 py-0.5 hover:bg-accent hover:text-accent-foreground"
          data-testid="open-palette"
          @click="palette && (palette.open = true)"
        >
          ⌘K
        </button>
        <button
          type="button"
          class="rounded px-1 py-0.5 hover:bg-accent hover:text-accent-foreground"
          :aria-label="mode === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'"
          data-testid="toggle-theme"
          @click="toggle()"
        >
          <component :is="mode === 'dark' ? Sun : Moon" class="size-3" />
        </button>
      </footer>
      <CommandPalette ref="palette" />
      <Toaster />
    </div>
  </TooltipProvider>
</template>
