<script setup lang="ts">
import type { Capability, ColumnFilter, TableInfo } from '@/lib/bindings'
import type { WorkspaceTab } from '@/lib/tabs'
import { ArrowLeft, KeyRound, Plus, RefreshCw, SquareTerminal, Table2, Unplug, X } from '@lucide/vue'
import { useEventListener } from '@vueuse/core'
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import KeyDetailPanel from '@/components/KeyDetailPanel.vue'
import KeyList from '@/components/KeyList.vue'
import RedisConsolePanel from '@/components/RedisConsolePanel.vue'
import SchemaTree from '@/components/SchemaTree.vue'
import SqlEditorPanel from '@/components/SqlEditorPanel.vue'
import TableGrid from '@/components/TableGrid.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useConnections } from '@/composables/useConnections'
import { useSchema } from '@/composables/useSchema'
import { useSqlSessions } from '@/composables/useSqlSessions'
import { useWorkspaceTabs } from '@/composables/useWorkspaceTabs'
import { commands } from '@/lib/bindings'
import { ENV_BADGE_CLASSES, serverBadge } from '@/lib/connections'
import { unwrap } from '@/lib/result'

const route = useRoute()
const router = useRouter()
const { connections, activeIds, refresh, connect, disconnect } = useConnections()
const { snapshots, pending, load, evict } = useSchema()
const sessions = useSqlSessions()

const id = computed(() => String(route.params.id))
const profile = computed(() => connections.value.find(p => p.id === id.value))
// The UI branches on capabilities, never on the kind itself.
const capabilities = ref<Capability[]>([])
const isKv = computed(() => capabilities.value.includes('kv-browse'))
const selectedKey = ref<string | null>(null)
const kvView = ref<'key' | 'console'>('key')
const keyList = useTemplateRef('keyList')

function onKeyDeleted() {
  selectedKey.value = null
  keyList.value?.refresh()
}
const snapshot = computed(() => snapshots.value[id.value])
const filter = ref('')
const serverVersion = ref<string | null>(null)
// "18.4 (Debian...)" -> "PG 18.4"; "11.4.7-MariaDB-log" -> "MariaDB 11.4.7".
const versionBadge = computed(() => {
  if (serverVersion.value === null || !profile.value)
    return null
  return serverBadge(profile.value.params.kind, serverVersion.value)
})
// sqlite's schema namespace is always "main"; servers use the profile database.
const database = computed(() => {
  const params = profile.value?.params
  if (!params || params.kind === 'redis')
    return ''
  return params.kind === 'sqlite' ? 'main' : params.database
})
const tabs = useWorkspaceTabs(String(route.params.id))
const activeTab = computed(() => tabs.state.value.tabs.find(tab => tab.id === tabs.state.value.activeId) ?? null)

function tableInfo(tab: WorkspaceTab): TableInfo | null {
  if (tab.type !== 'table')
    return null
  return snapshot.value?.schemas
    .find(s => s.name === tab.schema)
    ?.tables
    .find(t => t.name === tab.table) ?? null
}

const activeTableTitle = computed(() => {
  const tab = activeTab.value
  if (tab?.type !== 'table')
    return null
  const info = tableInfo(tab)
  return info ? { name: `${tab.schema}.${tab.table}`, columns: info.columns.length } : null
})

function tabLabel(tab: WorkspaceTab): string {
  return tab.type === 'table' ? `${tab.schema}.${tab.table}` : tab.title
}

function closeTab(tab: WorkspaceTab) {
  if (tab.type === 'sql')
    sessions.close(id.value, tab.id).catch(() => {})
  tabs.close(tab.id)
}

// Tab shortcuts: new sql, close, cycle. No tabs on the kv surface.
useEventListener('keydown', (event) => {
  if (!event.ctrlKey || event.defaultPrevented || isKv.value)
    return
  if (event.key === 't') {
    event.preventDefault()
    tabs.openSql()
  }
  else if (event.key === 'w' && activeTab.value) {
    event.preventDefault()
    closeTab(activeTab.value)
  }
  else if (event.key === 'Tab') {
    event.preventDefault()
    tabs.cycle(event.shiftKey ? -1 : 1)
  }
})

onMounted(async () => {
  await refresh()
  if (!profile.value) {
    router.push({ name: 'connections' })
    return
  }
  try {
    capabilities.value = unwrap(await commands.connectorCapabilities(profile.value.params.kind))
    if (!activeIds.value.has(id.value))
      await connect(id.value)
    serverVersion.value = unwrap(await commands.serverVersion(id.value))
    if (!isKv.value)
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
    sessions.evictConnection(id.value)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  router.push({ name: 'connections' })
}

function selectTable(schema: string, table: TableInfo) {
  tabs.openTable(schema, table.name)
}

function hop(schema: string, table: string, filters: ColumnFilter[]) {
  const target = snapshot.value?.schemas
    .find(s => s.name === schema)
    ?.tables
    .find(t => t.name === table)
  if (!target) {
    toast.error(`table ${schema}.${table} is not in the schema snapshot`)
    return
  }
  tabs.openTable(schema, table, filters)
}
</script>

<template>
  <ResizablePanelGroup direction="horizontal" auto-save-id="soquel-workspace" class="h-full min-h-0">
    <ResizablePanel id="workspace-sidebar" :default-size="20" :min-size="12" :max-size="40">
      <aside class="flex h-full min-h-0 flex-col bg-sidebar">
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
          <Badge
            v-if="versionBadge"
            variant="outline"
            class="border-transparent bg-muted font-mono text-[10px] text-muted-foreground"
            :title="`${versionBadge.engine} ${serverVersion}`"
            data-testid="server-version"
          >
            {{ versionBadge.engine }} {{ versionBadge.version }}
          </Badge>
          <Tooltip v-if="!isKv">
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
        <KeyList
          v-if="isKv"
          ref="keyList"
          :connection-id="id"
          :selected="selectedKey"
          @select="key => { selectedKey = key; kvView = 'key' }"
        />
        <template v-else>
          <div class="px-2 py-2">
            <Input v-model="filter" placeholder="filter tables" class="h-7 font-mono text-xs" data-testid="tree-filter" />
          </div>
          <ScrollArea class="min-h-0 flex-1">
            <SchemaTree v-if="snapshot" :snapshot="snapshot" :filter="filter" @select="selectTable" />
            <p v-else class="px-4 py-6 font-mono text-xs text-muted-foreground">
              loading schema…
            </p>
          </ScrollArea>
        </template>
      </aside>
    </ResizablePanel>

    <ResizableHandle with-handle />

    <ResizablePanel v-if="isKv" id="workspace-main" class="flex min-w-0 flex-col">
      <header class="flex items-center gap-0.5 border-b px-1.5 py-1 font-mono text-xs text-muted-foreground">
        <button
          type="button"
          class="flex cursor-pointer items-center gap-1.5 rounded px-2 py-0.5"
          :class="kvView === 'key' ? 'bg-accent text-accent-foreground' : 'hover:text-foreground'"
          data-testid="kv-view-key"
          @click="kvView = 'key'"
        >
          <KeyRound class="size-3 opacity-60" />
          key
        </button>
        <button
          type="button"
          class="flex cursor-pointer items-center gap-1.5 rounded px-2 py-0.5"
          :class="kvView === 'console' ? 'bg-accent text-accent-foreground' : 'hover:text-foreground'"
          data-testid="kv-view-console"
          @click="kvView = 'console'"
        >
          <SquareTerminal class="size-3 opacity-60" />
          console
        </button>
      </header>
      <KeyDetailPanel
        v-show="kvView === 'key'"
        :connection-id="id"
        :key-name="selectedKey"
        @deleted="onKeyDeleted"
        @changed="keyList?.refresh()"
      />
      <RedisConsolePanel
        v-show="kvView === 'console'"
        :connection-id="id"
        @ran="keyList?.refresh()"
      />
    </ResizablePanel>

    <ResizablePanel v-else id="workspace-main" class="flex min-w-0 flex-col">
      <header class="flex items-center border-b font-mono text-xs text-muted-foreground">
        <div class="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto px-1.5 py-1" data-testid="tab-bar">
          <button
            v-for="tab in tabs.state.value.tabs"
            :key="tab.id"
            type="button"
            class="group/tab flex shrink-0 cursor-pointer items-center gap-1.5 rounded px-2 py-0.5"
            :class="tab.id === tabs.state.value.activeId ? 'bg-accent text-accent-foreground' : 'hover:text-foreground'"
            :data-testid="`tab-${tabLabel(tab)}`"
            :data-active="tab.id === tabs.state.value.activeId || undefined"
            @click="tabs.activate(tab.id)"
            @mousedown.middle.prevent="closeTab(tab)"
          >
            <component :is="tab.type === 'table' ? Table2 : SquareTerminal" class="size-3 shrink-0 opacity-60" />
            {{ tabLabel(tab) }}
            <span
              class="rounded text-muted-foreground opacity-0 group-hover/tab:opacity-100 hover:text-foreground"
              :class="tab.id === tabs.state.value.activeId && 'opacity-100'"
              role="button"
              :aria-label="`Close ${tabLabel(tab)}`"
              :data-testid="`close-tab-${tabLabel(tab)}`"
              @click.stop="closeTab(tab)"
            >
              <X class="size-3" />
            </span>
          </button>
          <Tooltip>
            <TooltipTrigger as-child>
              <button
                type="button"
                class="flex shrink-0 cursor-pointer items-center rounded px-1.5 py-1 hover:text-foreground"
                aria-label="New sql editor"
                data-testid="new-sql-tab"
                @click="tabs.openSql()"
              >
                <Plus class="size-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent>New sql editor (ctrl+t)</TooltipContent>
          </Tooltip>
        </div>
        <div v-if="activeTableTitle" class="flex shrink-0 items-center gap-2 px-3">
          <span data-testid="table-title">{{ activeTableTitle.name }}</span>
          <span class="text-muted-foreground/60">{{ activeTableTitle.columns }} columns</span>
        </div>
      </header>

      <template v-for="tab in tabs.state.value.tabs" :key="tab.id">
        <TableGrid
          v-if="tab.type === 'table' && tableInfo(tab)"
          v-show="tab.id === tabs.state.value.activeId"
          class="min-h-0 flex-1"
          :connection-id="id"
          :kind="profile?.params.kind ?? 'postgres'"
          :schema="tab.schema"
          :table="tableInfo(tab)!"
          :initial-filters="tab.initialFilters"
          @hop="hop"
        />
        <div
          v-else-if="tab.type === 'table'"
          v-show="tab.id === tabs.state.value.activeId"
          class="flex flex-1 items-center justify-center gap-3 font-mono text-xs text-muted-foreground"
        >
          {{ tab.schema }}.{{ tab.table }} is gone from the schema
          <Button size="sm" variant="secondary" @click="closeTab(tab)">
            Close tab
          </Button>
        </div>
        <SqlEditorPanel
          v-else-if="tab.type === 'sql'"
          v-show="tab.id === tabs.state.value.activeId"
          class="min-h-0 flex-1"
          :connection-id="id"
          :kind="profile?.params.kind ?? 'postgres'"
          :database="database"
          :tab-id="tab.id"
          :snapshot="snapshot"
        />
      </template>
      <div
        v-if="tabs.state.value.tabs.length === 0"
        class="flex flex-1 items-center justify-center text-muted-foreground"
      >
        <p class="font-mono text-sm">
          soquel=#<span class="ml-1 text-muted-foreground/60">select a table or open sql</span>
        </p>
      </div>
    </ResizablePanel>
  </ResizablePanelGroup>
</template>
