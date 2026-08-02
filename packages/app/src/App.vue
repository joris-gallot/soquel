<script setup lang="ts">
import { Moon, Sun } from '@lucide/vue'
import { getVersion } from '@tauri-apps/api/app'
import { useAsyncState } from '@vueuse/core'
import { TooltipProvider } from 'reka-ui'
import { onMounted, ref } from 'vue'
import CommandApprovalDialog from '@/components/CommandApprovalDialog.vue'
import CommandPalette from '@/components/CommandPalette.vue'
import HostKeyDialog from '@/components/HostKeyDialog.vue'
import McpApprovalDialog from '@/components/McpApprovalDialog.vue'
import SecretPromptDialog from '@/components/SecretPromptDialog.vue'
import { Toaster } from '@/components/ui/sonner'
import UpdatePanel from '@/components/UpdatePanel.vue'
import { useTheme } from '@/composables/useTheme'
import { useUpdater } from '@/composables/useUpdater'

const { mode, toggle } = useTheme()

const palette = ref<InstanceType<typeof CommandPalette> | null>(null)

const { state: version } = useAsyncState(getVersion, '')

const { available, panelOpen, check, listen } = useUpdater()

onMounted(async () => {
  await listen()
  await check()
})
</script>

<template>
  <TooltipProvider :delay-duration="300">
    <div class="flex h-screen flex-col">
      <div class="min-h-0 flex-1">
        <!-- Keyed: workspace->workspace navigation must remount (connect + per-connection tabs). -->
        <RouterView v-slot="{ Component }">
          <component :is="Component" :key="$route.fullPath" />
        </RouterView>
      </div>
      <footer class="flex items-center gap-2 border-t px-4 py-1 font-mono text-[11px] text-muted-foreground">
        <button
          v-if="available"
          type="button"
          class="flex items-center gap-1.5 rounded px-1 py-0.5 hover:bg-accent hover:text-accent-foreground"
          data-testid="app-version"
          :aria-label="`Update to ${available.version}`"
          @click="panelOpen = true"
        >
          soquel {{ version }}
          <span class="size-1.5 rounded-full bg-primary" />
        </button>
        <span v-else data-testid="app-version">soquel {{ version }}</span>
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
      <HostKeyDialog />
      <SecretPromptDialog />
      <CommandApprovalDialog />
      <McpApprovalDialog />
      <UpdatePanel v-model:open="panelOpen" />
      <Toaster />
    </div>
  </TooltipProvider>
</template>
