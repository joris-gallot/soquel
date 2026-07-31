<script setup lang="ts">
import { Check, Copy, RefreshCw, ScrollText } from '@lucide/vue'
import { useClipboard } from '@vueuse/core'
import { computed, onMounted, ref } from 'vue'
import { toast } from 'vue-sonner'
import McpAuditDialog from '@/components/McpAuditDialog.vue'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useConnections } from '@/composables/useConnections'
import { useMcp } from '@/composables/useMcp'

const { status, refresh, start, stop, regenerateToken } = useMcp()
const { connections } = useConnections()

const exposedCount = computed(() =>
  connections.value.filter(profile => (profile.agentAccess ?? 'none') !== 'none').length,
)

function setupCommand(token: string) {
  if (!status.value)
    return ''
  return `claude mcp add --transport http ${status.value.serverName} ${status.value.endpoint} --header "Authorization: Bearer ${token}"`
}

const busy = ref(false)
const auditOpen = ref(false)
const connectionNames = computed(() =>
  Object.fromEntries(connections.value.map(profile => [profile.id, profile.name])),
)
const { copy, copied } = useClipboard({ legacy: true })

onMounted(refresh)

async function toggle(on: boolean) {
  busy.value = true
  try {
    if (on)
      await start()
    else
      await stop()
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    busy.value = false
  }
}

async function copySetup() {
  if (!status.value)
    return
  await copy(setupCommand(status.value.token))
  toast.success('Setup command copied')
}

async function regenerate() {
  try {
    await regenerateToken()
    toast.success('Token regenerated. Agents need the new setup command.')
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}
</script>

<template>
  <section class="mt-10">
    <header class="mb-4 flex items-center justify-between">
      <h2 class="font-mono text-sm text-muted-foreground">
        agent access (mcp)
      </h2>
      <Button size="sm" variant="secondary" data-testid="open-audit" @click="auditOpen = true">
        <ScrollText />
        Activity
      </Button>
    </header>

    <div class="rounded-lg border">
      <div class="flex items-center gap-3 px-4 py-3">
        <span
          class="size-2 shrink-0 rounded-full"
          :class="status?.running ? 'bg-[oklch(0.72_0.11_240)]' : 'bg-muted-foreground/30'"
          :data-testid="status?.running ? 'mcp-running' : 'mcp-stopped'"
        />
        <div class="min-w-0 flex-1">
          <span class="text-sm font-medium">MCP server</span>
          <p class="truncate font-mono text-xs text-muted-foreground">
            {{ status?.running ? status.endpoint : 'Give coding agents scoped access to your connections.' }}
          </p>
        </div>
        <Switch
          :model-value="status?.running ?? false"
          data-testid="mcp-toggle"
          :disabled="!status || busy"
          aria-label="MCP server"
          @update:model-value="toggle"
        />
      </div>

      <div
        v-if="status?.running"
        class="space-y-3 border-t px-4 py-3"
        data-testid="mcp-details"
        :data-token="status.token"
      >
        <p class="text-xs text-muted-foreground">
          {{ exposedCount === 0
            ? 'No connections are exposed yet. Opt one in with the Agent access field in its settings.'
            : `${exposedCount} ${exposedCount === 1 ? 'connection' : 'connections'} exposed, read-only. Every agent query lands in the audit log.` }}
        </p>
        <div class="flex items-center gap-2">
          <code class="min-w-0 flex-1 truncate rounded bg-muted/40 px-2 py-1.5 font-mono text-xs">
            {{ setupCommand('••••••••') }}
          </code>
          <Button size="sm" variant="secondary" data-testid="mcp-copy-setup" @click="copySetup">
            <component :is="copied ? Check : Copy" />
            Copy
          </Button>
        </div>
        <p class="text-xs text-muted-foreground">
          The command is for Claude Code; any MCP client over streamable HTTP works with the same URL and token.
        </p>
      </div>

      <div v-else-if="status" class="flex items-center justify-between border-t px-4 py-3">
        <p class="text-xs text-muted-foreground">
          Off. Connections stay invisible to agents until the server runs.
        </p>
        <Button size="sm" variant="ghost" data-testid="mcp-regenerate" @click="regenerate">
          <RefreshCw />
          Regenerate token
        </Button>
      </div>
    </div>

    <McpAuditDialog v-model:open="auditOpen" :names="connectionNames" />
  </section>
</template>
