<script setup lang="ts">
import type { ConnectionProfile, TunnelProfile } from '@/lib/bindings'
import { Bot, Cable, ChevronDown, ChevronRight, MoreHorizontal, Plug, Plus, Unplug } from '@lucide/vue'
import { useLocalStorage } from '@vueuse/core'
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import ConnectionFormDialog from '@/components/ConnectionFormDialog.vue'
import McpServerPanel from '@/components/McpServerPanel.vue'
import TunnelFormDialog from '@/components/TunnelFormDialog.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useConnections } from '@/composables/useConnections'
import { useTunnels } from '@/composables/useTunnels'
import { connectionDsn, ENV_BADGE_CLASSES, groupConnections } from '@/lib/connections'
import { CommandError } from '@/lib/result'
import { SSH_AUTH_LABELS } from '@/lib/tunnels'

const router = useRouter()
const { connections, activeIds, refresh, remove, connect, disconnect } = useConnections()
const { tunnels, refresh: refreshTunnels, remove: removeTunnel } = useTunnels()

const sections = computed(() => groupConnections(connections.value))
const collapsed = useLocalStorage<string[]>('soquel:collapsed-groups', [])

function toggleGroup(group: string) {
  collapsed.value = collapsed.value.includes(group)
    ? collapsed.value.filter(name => name !== group)
    : [...collapsed.value, group]
}

const formOpen = ref(false)
const editing = ref<ConnectionProfile | null>(null)
const busyId = ref<string | null>(null)
const tunnelFormOpen = ref(false)
const editingTunnel = ref<TunnelProfile | null>(null)

onMounted(() => {
  refresh()
  refreshTunnels()
})

function openCreate() {
  editing.value = null
  formOpen.value = true
}

function openEdit(profile: ConnectionProfile) {
  editing.value = profile
  formOpen.value = true
}

async function toggle(profile: ConnectionProfile) {
  busyId.value = profile.id
  try {
    if (activeIds.value.has(profile.id)) {
      await disconnect(profile.id)
    }
    else {
      await connect(profile.id)
      router.push({ name: 'workspace', params: { id: profile.id } })
    }
  }
  catch (error) {
    // The host-key trust dialog owns this failure mode.
    if (!(error instanceof CommandError && error.kind === 'host-key-untrusted'))
      toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    busyId.value = null
  }
}

async function openWorkspace(profile: ConnectionProfile) {
  if (!activeIds.value.has(profile.id)) {
    await toggle(profile)
    return
  }
  router.push({ name: 'workspace', params: { id: profile.id } })
}

async function removeProfile(profile: ConnectionProfile) {
  try {
    await remove(profile.id)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

function openCreateTunnel() {
  editingTunnel.value = null
  tunnelFormOpen.value = true
}

function openEditTunnel(tunnel: TunnelProfile) {
  editingTunnel.value = tunnel
  tunnelFormOpen.value = true
}

async function removeTunnelProfile(tunnel: TunnelProfile) {
  try {
    await removeTunnel(tunnel.id)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}
</script>

<template>
  <div class="mx-auto w-full max-w-3xl px-6 py-8">
    <header class="mb-6 flex items-center justify-between">
      <h1 class="font-mono text-sm text-muted-foreground">
        connections
      </h1>
      <Button size="sm" data-testid="new-connection" @click="openCreate">
        <Plus />
        New connection
      </Button>
    </header>

    <div
      v-if="connections.length === 0"
      data-testid="empty-state"
      class="flex flex-col items-center gap-4 rounded-lg border border-dashed py-20"
    >
      <p class="font-mono text-2xl tracking-tight">
        <span class="font-semibold text-foreground">soquel</span><span class="text-muted-foreground">=#</span>
        <span class="prompt-cursor ml-1.5 inline-block h-5 w-2.5 translate-y-0.5 bg-[oklch(0.72_0.11_240)]" aria-hidden="true" />
      </p>
      <p class="text-sm text-muted-foreground">
        No connections yet. Add your first database.
      </p>
      <Button size="sm" variant="secondary" @click="openCreate">
        <Plus />
        Add connection
      </Button>
    </div>

    <div v-else class="divide-y rounded-lg border">
      <Collapsible
        v-for="section in sections"
        :key="section.group ?? ''"
        :open="section.group === null || !collapsed.includes(section.group)"
        @update:open="section.group !== null && toggleGroup(section.group)"
      >
        <CollapsibleTrigger
          v-if="section.group !== null"
          class="flex w-full cursor-pointer items-center gap-1.5 bg-muted/40 px-3 py-1.5 font-mono text-xs text-muted-foreground hover:text-foreground"
          :data-testid="`group-${section.group}`"
        >
          <component :is="collapsed.includes(section.group) ? ChevronRight : ChevronDown" class="size-3" />
          {{ section.group }}
          <span class="text-muted-foreground/60">({{ section.profiles.length }})</span>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <ul class="divide-y">
            <li
              v-for="profile in section.profiles"
              :key="profile.id"
              data-testid="connection-row"
              class="flex items-center gap-3 px-4 py-3"
            >
              <span
                class="size-2 shrink-0 rounded-full"
                :class="activeIds.has(profile.id) ? 'bg-[oklch(0.72_0.11_240)]' : 'bg-muted-foreground/30'"
                :data-testid="activeIds.has(profile.id) ? 'status-connected' : 'status-disconnected'"
              />
              <button
                type="button"
                class="min-w-0 flex-1 text-left"
                :data-testid="`open-${profile.name}`"
                @click="openWorkspace(profile)"
              >
                <div class="flex items-center gap-2">
                  <span class="truncate text-sm font-medium">{{ profile.name }}</span>
                  <Badge variant="outline" class="font-mono text-[10px]" :class="ENV_BADGE_CLASSES[profile.env]">
                    {{ profile.env }}
                  </Badge>
                  <Badge
                    v-if="(profile.agentAccess ?? 'none') !== 'none'"
                    variant="outline"
                    class="gap-1 font-mono text-[10px]"
                    data-testid="agent-badge"
                  >
                    <Bot class="size-2.5" />
                    agent
                  </Badge>
                </div>
                <p class="truncate font-mono text-xs text-muted-foreground">
                  {{ connectionDsn(profile.params) }}
                </p>
              </button>
              <Button
                size="sm"
                variant="ghost"
                data-testid="toggle-connection"
                :disabled="busyId === profile.id"
                @click="toggle(profile)"
              >
                <component :is="activeIds.has(profile.id) ? Unplug : Plug" />
                {{ activeIds.has(profile.id) ? 'Disconnect' : 'Connect' }}
              </Button>
              <DropdownMenu>
                <DropdownMenuTrigger as-child>
                  <Button size="icon-sm" variant="ghost" data-testid="row-menu" aria-label="Connection actions">
                    <MoreHorizontal />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem data-testid="row-edit" @click="openEdit(profile)">
                    Edit
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem data-testid="row-delete" variant="destructive" @click="removeProfile(profile)">
                    Delete
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </li>
          </ul>
        </CollapsibleContent>
      </Collapsible>
    </div>

    <section class="mt-10">
      <header class="mb-4 flex items-center justify-between">
        <h2 class="font-mono text-sm text-muted-foreground">
          ssh tunnels
        </h2>
        <Button size="sm" variant="secondary" data-testid="new-tunnel" @click="openCreateTunnel">
          <Plus />
          New tunnel
        </Button>
      </header>

      <p v-if="tunnels.length === 0" class="text-sm text-muted-foreground">
        No tunnels. Reach databases behind a bastion by referencing a tunnel from a connection.
      </p>

      <ul v-else class="divide-y rounded-lg border">
        <li
          v-for="tunnel in tunnels"
          :key="tunnel.id"
          data-testid="tunnel-row"
          class="flex items-center gap-3 px-4 py-3"
        >
          <Cable class="size-4 shrink-0 text-muted-foreground" />
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate text-sm font-medium">{{ tunnel.name }}</span>
              <Badge variant="outline" class="font-mono text-[10px]">
                {{ SSH_AUTH_LABELS[tunnel.auth.method] }}
              </Badge>
            </div>
            <p class="truncate font-mono text-xs text-muted-foreground">
              ssh://{{ tunnel.user }}@{{ tunnel.host }}:{{ tunnel.port }}
            </p>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <Button size="icon-sm" variant="ghost" data-testid="tunnel-menu" aria-label="Tunnel actions">
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem data-testid="tunnel-edit" @click="openEditTunnel(tunnel)">
                Edit
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem data-testid="tunnel-delete" variant="destructive" @click="removeTunnelProfile(tunnel)">
                Delete
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </li>
      </ul>
    </section>

    <McpServerPanel />

    <ConnectionFormDialog v-model:open="formOpen" :profile="editing" />
    <TunnelFormDialog v-model:open="tunnelFormOpen" :tunnel="editingTunnel" />
  </div>
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
