<script setup lang="ts">
import { Moon, Plug, Plus, Sun } from '@lucide/vue'
import { useMagicKeys, whenever } from '@vueuse/core'
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from '@/components/ui/command'
import { useConnections } from '@/composables/useConnections'
import { useTheme } from '@/composables/useTheme'
import { groupConnections } from '@/lib/connections'

const router = useRouter()
const { connections, connect, activeIds } = useConnections()
const { mode, toggle } = useTheme()

const sections = computed(() => groupConnections(connections.value))

const open = ref(false)

const keys = useMagicKeys({
  passive: false,
  onEventFired(event) {
    if ((event.ctrlKey || event.metaKey) && event.key === 'k')
      event.preventDefault()
  },
})
whenever(keys['ctrl+k']!, () => {
  open.value = !open.value
})
whenever(keys['meta+k']!, () => {
  open.value = !open.value
})

async function quickConnect(id: string) {
  open.value = false
  try {
    if (!activeIds.value.has(id))
      await connect(id)
    router.push({ name: 'workspace', params: { id } })
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

function newConnection() {
  open.value = false
  router.push({ name: 'connections' })
}

function toggleTheme() {
  open.value = false
  toggle()
}

defineExpose({ open })
</script>

<template>
  <CommandDialog v-model:open="open">
    <CommandInput placeholder="Search connections and actions…" data-testid="palette-input" />
    <CommandList>
      <CommandEmpty>Nothing found.</CommandEmpty>
      <CommandGroup
        v-for="section in sections"
        :key="section.group ?? ''"
        :heading="section.group ?? 'Connections'"
      >
        <CommandItem
          v-for="profile in section.profiles"
          :key="profile.id"
          :value="`connect ${section.group ?? ''} ${profile.name} ${profile.host} ${profile.database}`"
          @select="quickConnect(profile.id)"
        >
          <Plug />
          <span>{{ profile.name }}</span>
          <span class="ml-auto font-mono text-xs text-muted-foreground">
            {{ profile.host }}:{{ profile.port }}/{{ profile.database }}
          </span>
        </CommandItem>
      </CommandGroup>
      <CommandSeparator />
      <CommandGroup heading="Actions">
        <CommandItem value="new connection" @select="newConnection">
          <Plus />
          <span>New connection</span>
        </CommandItem>
        <CommandItem value="toggle theme dark light" @select="toggleTheme">
          <component :is="mode === 'dark' ? Sun : Moon" />
          <span>Switch to {{ mode === 'dark' ? 'light' : 'dark' }} theme</span>
        </CommandItem>
      </CommandGroup>
    </CommandList>
  </CommandDialog>
</template>
