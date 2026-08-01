<script setup lang="ts">
import type { ConnectionProfile, ImportSource, SecretSubject, TunnelProfile } from '@/lib/bindings'
import { Bot, Cable, ChevronDown, ChevronRight, Download, MoreHorizontal, Plug, Plus, Unplug, Upload } from '@lucide/vue'
import { useLocalStorage } from '@vueuse/core'
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import ConnectionFormDialog from '@/components/ConnectionFormDialog.vue'
import ExportConnectionsDialog from '@/components/ExportConnectionsDialog.vue'
import ImportConnectionsDialog from '@/components/ImportConnectionsDialog.vue'
import ImportSourcesDialog from '@/components/ImportSourcesDialog.vue'
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
import { useCommandApproval } from '@/composables/useCommandApproval'
import { useConnections } from '@/composables/useConnections'
import { useSecretPrompt } from '@/composables/useSecretPrompt'
import { useTunnels } from '@/composables/useTunnels'
import { commands, events } from '@/lib/bindings'
import { connectionDsn, ENV_BADGE_CLASSES, groupConnections } from '@/lib/connections'
import { CommandError, unwrap } from '@/lib/result'
import { soquelFileSource } from '@/lib/transfer'
import { SSH_AUTH_LABELS } from '@/lib/tunnels'

const HANDLED_BY_A_DIALOG = ['host-key-untrusted', 'secret-required', 'command-approval-required']

const router = useRouter()
const { connections, activeIds, refresh, remove, connect, disconnect } = useConnections()
const { tunnels, refresh: refreshTunnels, remove: removeTunnel } = useTunnels()
const { intercept: interceptSecret } = useSecretPrompt()
const { intercept: interceptCommand } = useCommandApproval()

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
const exportOpen = ref(false)
const importOpen = ref(false)
const importSourcesOpen = ref(false)
const importSource = ref<ImportSource | null>(null)

/// Puts the command back in waiting: the next connect asks again.
async function revokeCommand(subject: SecretSubject, id: string) {
  try {
    unwrap(await commands.revokeCredentialCommand(subject, id))
    toast.success('The command will ask before running again')
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

function startImport() {
  importSourcesOpen.value = true
}

function importFrom(source: ImportSource) {
  importSource.value = source
  importOpen.value = true
}

async function afterImport() {
  await refresh()
  await refreshTunnels()
}

onMounted(async () => {
  refresh()
  refreshTunnels()
  // A file handed to the app from outside the webview (opened from the OS,
  // dropped on the window) lands here and opens the dialog on it.
  await events.importFileRequested.listen(({ payload }) => {
    importFrom(soquelFileSource(payload.path))
  })
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
    // Retry the whole gesture, navigation included, once the password lands.
    interceptSecret(error, () => toggle(profile))
    interceptCommand(error, () => toggle(profile))
    // The trust and password dialogs own these failure modes.
    if (!(error instanceof CommandError && HANDLED_BY_A_DIALOG.includes(error.kind)))
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
      <div class="flex items-center gap-1.5">
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button size="icon-sm" variant="ghost" data-testid="connections-menu" aria-label="Connection list actions">
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <!-- The content defaults to the trigger width: an icon button would wrap these labels. -->
          <DropdownMenuContent align="end" class="w-auto!">
            <DropdownMenuItem data-testid="open-import" @click="startImport">
              <Upload />
              Import connections…
            </DropdownMenuItem>
            <DropdownMenuItem
              data-testid="open-export"
              :disabled="connections.length === 0"
              @click="exportOpen = true"
            >
              <Download />
              Export connections…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button size="sm" data-testid="new-connection" @click="openCreate">
          <Plus />
          New connection
        </Button>
      </div>
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
      <div class="flex items-center gap-2">
        <Button size="sm" variant="secondary" @click="openCreate">
          <Plus />
          Add connection
        </Button>
        <Button size="sm" variant="ghost" data-testid="empty-import" @click="startImport">
          <Upload />
          Import a file
        </Button>
      </div>
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
                  <DropdownMenuItem
                    v-if="profile.credential?.mode === 'command'"
                    data-testid="row-revoke-command"
                    @click="revokeCommand('connection', profile.id)"
                  >
                    Revoke the credential command
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
              <DropdownMenuItem
                v-if="tunnel.credential?.mode === 'command'"
                data-testid="tunnel-revoke-command"
                @click="revokeCommand('tunnel', tunnel.id)"
              >
                Revoke the credential command
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
    <ExportConnectionsDialog v-model:open="exportOpen" />
    <ImportSourcesDialog v-model:open="importSourcesOpen" @chosen="importFrom" />
    <ImportConnectionsDialog v-model:open="importOpen" :source="importSource" @imported="afterImport" />
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
