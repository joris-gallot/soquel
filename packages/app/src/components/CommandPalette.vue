<script setup lang="ts">
import { ArrowDownToLine, ClipboardList, FolderOpen, Moon, Plug, Plus, Sun } from '@lucide/vue'
import { useClipboard, useMagicKeys, whenever } from '@vueuse/core'
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
  CommandShortcut,
} from '@/components/ui/command'
import { useCommandApproval } from '@/composables/useCommandApproval'
import { useConnections } from '@/composables/useConnections'
import { useSecretPrompt } from '@/composables/useSecretPrompt'
import { useTheme } from '@/composables/useTheme'
import { useUpdater } from '@/composables/useUpdater'
import { commands } from '@/lib/bindings'
import { connectionTarget, groupConnections } from '@/lib/connections'
import { unwrap } from '@/lib/result'

const router = useRouter()
const { connections, connect, activeIds } = useConnections()
const { intercept: interceptSecret } = useSecretPrompt()
const { intercept: interceptCommand } = useCommandApproval()
const { mode, toggle } = useTheme()
const { panelOpen, check: checkForUpdate } = useUpdater()
const { copy } = useClipboard({ legacy: true })

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
    if (interceptSecret(error, () => quickConnect(id)) || interceptCommand(error, () => quickConnect(id)))
      return
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

async function copyDiagnostics() {
  open.value = false
  try {
    await copy(unwrap(await commands.diagnostics()))
    toast.success('Diagnostics copied. It carries no connection names.')
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

async function openLogFolder() {
  open.value = false
  try {
    unwrap(await commands.openLogFolder())
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

async function checkForUpdates() {
  open.value = false
  if (await checkForUpdate())
    panelOpen.value = true
  else
    toast.success('soquel is up to date.')
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
          :value="`connect ${section.group ?? ''} ${profile.name} ${connectionTarget(profile.params)}`"
          @select="quickConnect(profile.id)"
        >
          <Plug />
          <span>{{ profile.name }}</span>
          <!-- CommandShortcut: the slot CommandItem right-aligns, hiding its
               trailing check icon that would otherwise split the free space. -->
          <CommandShortcut class="font-mono tracking-normal">
            {{ connectionTarget(profile.params) }}
          </CommandShortcut>
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
        <CommandItem value="check for updates" @select="checkForUpdates">
          <ArrowDownToLine />
          <span>Check for updates</span>
        </CommandItem>
        <CommandItem value="copy diagnostics support bug report" @select="copyDiagnostics">
          <ClipboardList />
          <span>Copy diagnostics</span>
        </CommandItem>
        <CommandItem value="open log folder logs" @select="openLogFolder">
          <FolderOpen />
          <span>Open log folder</span>
        </CommandItem>
      </CommandGroup>
    </CommandList>
  </CommandDialog>
</template>
