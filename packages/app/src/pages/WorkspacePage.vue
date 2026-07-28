<script setup lang="ts">
import type { TableInfo } from '@/lib/bindings'
import { ArrowLeft, RefreshCw, Unplug } from '@lucide/vue'
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import SchemaTree from '@/components/SchemaTree.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useConnections } from '@/composables/useConnections'
import { useSchema } from '@/composables/useSchema'
import { ENV_BADGE_CLASSES } from '@/lib/connections'

const route = useRoute()
const router = useRouter()
const { connections, activeIds, refresh, connect, disconnect } = useConnections()
const { snapshots, pending, load, evict } = useSchema()

const id = computed(() => String(route.params.id))
const profile = computed(() => connections.value.find(p => p.id === id.value))
const snapshot = computed(() => snapshots.value[id.value])
const filter = ref('')
const selectedTable = ref<{ schema: string, table: TableInfo } | null>(null)

onMounted(async () => {
  await refresh()
  if (!profile.value) {
    router.push({ name: 'connections' })
    return
  }
  try {
    if (!activeIds.value.has(id.value))
      await connect(id.value)
    await load(id.value)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
    router.push({ name: 'connections' })
  }
})

async function refreshSchema() {
  try {
    await load(id.value, true)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

async function leave() {
  try {
    await disconnect(id.value)
    evict(id.value)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  router.push({ name: 'connections' })
}

function selectTable(schema: string, table: TableInfo) {
  selectedTable.value = { schema, table }
}
</script>

<template>
  <div class="flex h-full min-h-0">
    <aside class="flex w-64 shrink-0 flex-col border-r bg-sidebar">
      <header class="flex items-center gap-1.5 border-b px-2 py-2">
        <Tooltip>
          <TooltipTrigger as-child>
            <Button size="icon-sm" variant="ghost" aria-label="Back to connections" data-testid="workspace-back" @click="router.push({ name: 'connections' })">
              <ArrowLeft />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Connections</TooltipContent>
        </Tooltip>
        <span class="min-w-0 flex-1 truncate font-mono text-sm" data-testid="workspace-name">
          {{ profile?.name }}
        </span>
        <Badge
          v-if="profile"
          variant="outline"
          class="font-mono text-[10px]"
          :class="ENV_BADGE_CLASSES[profile.env]"
        >
          {{ profile.env }}
        </Badge>
        <Tooltip>
          <TooltipTrigger as-child>
            <Button
              size="icon-sm"
              variant="ghost"
              aria-label="Refresh schema"
              data-testid="refresh-schema"
              :disabled="pending[id]"
              @click="refreshSchema"
            >
              <RefreshCw :class="pending[id] ? 'animate-spin' : ''" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Refresh schema</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger as-child>
            <Button size="icon-sm" variant="ghost" aria-label="Disconnect" data-testid="workspace-disconnect" @click="leave">
              <Unplug />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Disconnect</TooltipContent>
        </Tooltip>
      </header>
      <div class="px-2 py-2">
        <Input v-model="filter" placeholder="filter tables" class="h-7 font-mono text-xs" data-testid="tree-filter" />
      </div>
      <ScrollArea class="min-h-0 flex-1">
        <SchemaTree v-if="snapshot" :snapshot="snapshot" :filter="filter" @select="selectTable" />
        <p v-else class="px-4 py-6 font-mono text-xs text-muted-foreground">
          loading schema…
        </p>
      </ScrollArea>
    </aside>

    <main class="flex min-w-0 flex-1 items-center justify-center">
      <div v-if="selectedTable" class="text-center" data-testid="table-placeholder">
        <p class="font-mono text-lg">
          {{ selectedTable.schema }}.{{ selectedTable.table.name }}
        </p>
        <p class="mt-1 text-sm text-muted-foreground">
          {{ selectedTable.table.columns.length }} columns - table browser coming soon
        </p>
      </div>
      <div v-else class="text-center text-muted-foreground">
        <p class="font-mono text-sm">
          soquel=#<span class="ml-1 text-muted-foreground/60">select a table</span>
        </p>
      </div>
    </main>
  </div>
</template>
