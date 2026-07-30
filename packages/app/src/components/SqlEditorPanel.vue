<script setup lang="ts">
import type { ExportFormat, QueryResult, SchemaSnapshot } from '@/lib/bindings'
import type { HistoryEntry } from '@/lib/query-history'
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { PostgreSQL, sql } from '@codemirror/lang-sql'
import { Compartment, EditorState } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, placeholder } from '@codemirror/view'
import { History, OctagonX, Play } from '@lucide/vue'
import { useClipboard, useLocalStorage } from '@vueuse/core'
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { toast } from 'vue-sonner'
import ExportMenu from '@/components/ExportMenu.vue'
import ResultsTable from '@/components/ResultsTable.vue'
import { Button } from '@/components/ui/button'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useSqlSessions } from '@/composables/useSqlSessions'
import { commands } from '@/lib/bindings'
import { soquelEditorTheme } from '@/lib/codemirror'
import { EXPORT_FORMATS, pickExportPath } from '@/lib/export'
import { pushHistory } from '@/lib/query-history'
import { unwrap } from '@/lib/result'
import { DEFAULT_SCHEMA, snapshotToNamespace } from '@/lib/sql-schema'

const props = defineProps<{ connectionId: string, tabId: string, snapshot?: SchemaSnapshot | null }>()

const { run, cancel } = useSqlSessions()

const host = useTemplateRef('host')
let view: EditorView | undefined
const language = new Compartment()

// Tab ids persist across restarts, so drafts follow their tab.
const doc = useLocalStorage(`soquel:editor:${props.connectionId}:${props.tabId}`, '')
const historyEntries = useLocalStorage<HistoryEntry[]>(`soquel:history:${props.connectionId}`, [])

const running = ref(false)
const result = ref<QueryResult | null>(null)
const error = ref<string | null>(null)
const hasSelection = ref(false)
const historyOpen = ref(false)
const activeStatement = ref('0')

const rowsAffected = computed(() =>
  result.value?.statements.reduce((total, statement) => total + (statement.rowsAffected ?? 0), 0) ?? 0,
)

const { copy } = useClipboard()

const activeResult = computed(() => {
  const statements = result.value?.statements ?? []
  return statements.length === 1 ? statements[0] : statements[Number(activeStatement.value)] ?? null
})

async function copyResult(format: ExportFormat) {
  const statement = activeResult.value
  if (!statement)
    return
  try {
    const text = unwrap(await commands.formatStatement(statement.columns, statement.rows, format, 'results'))
    await copy(text)
    toast.success(`copied ${statement.rows.length} row${statement.rows.length === 1 ? '' : 's'} as ${EXPORT_FORMATS[format].label}`)
  }
  catch (caught) {
    toast.error(caught instanceof Error ? caught.message : String(caught))
  }
}

async function saveResult(format: ExportFormat) {
  const statement = activeResult.value
  if (!statement)
    return
  const path = await pickExportPath(format, 'results')
  if (path === null)
    return
  try {
    unwrap(await commands.exportStatement(statement.columns, statement.rows, format, 'results', path))
    toast.success(`exported ${statement.rows.length} row${statement.rows.length === 1 ? '' : 's'} to ${path}`)
  }
  catch (caught) {
    toast.error(caught instanceof Error ? caught.message : String(caught))
  }
}

function sqlExtension() {
  return sql({
    dialect: PostgreSQL,
    schema: props.snapshot ? snapshotToNamespace(props.snapshot) : undefined,
    defaultSchema: DEFAULT_SCHEMA,
  })
}

onMounted(() => {
  view = new EditorView({
    parent: host.value!,
    state: EditorState.create({
      doc: doc.value,
      extensions: [
        lineNumbers(),
        history(),
        placeholder('select … (ctrl+enter to run)'),
        keymap.of([
          {
            key: 'Mod-Enter',
            run: () => {
              runQuery()
              return true
            },
          },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        language.of(sqlExtension()),
        soquelEditorTheme(),
        EditorView.updateListener.of((update) => {
          if (update.docChanged)
            doc.value = update.state.doc.toString()
          if (update.selectionSet)
            hasSelection.value = !update.state.selection.main.empty
        }),
      ],
    }),
  })
})

onBeforeUnmount(() => view?.destroy())

watch(() => props.snapshot, () => {
  view?.dispatch({ effects: language.reconfigure(sqlExtension()) })
})

function currentSql(): string {
  if (!view)
    return ''
  const selection = view.state.selection.main
  return selection.empty
    ? view.state.doc.toString()
    : view.state.sliceDoc(selection.from, selection.to)
}

async function runQuery() {
  const statementSql = currentSql().trim()
  if (statementSql === '' || running.value)
    return
  running.value = true
  error.value = null
  try {
    result.value = await run(props.connectionId, props.tabId, statementSql)
    activeStatement.value = '0'
    historyEntries.value = pushHistory(historyEntries.value, {
      sql: statementSql,
      at: Date.now(),
      durationMs: result.value.durationMs ?? 0,
      ok: true,
    })
  }
  catch (caught) {
    error.value = caught instanceof Error ? caught.message : String(caught)
    historyEntries.value = pushHistory(historyEntries.value, {
      sql: statementSql,
      at: Date.now(),
      durationMs: 0,
      ok: false,
    })
  }
  finally {
    running.value = false
  }
}

async function cancelQuery() {
  try {
    await cancel(props.connectionId, props.tabId)
  }
  catch {
    // The query may have finished in the meantime; the run itself reports.
  }
}

function formatTime(at: number) {
  return new Date(at).toLocaleTimeString()
}

function loadFromHistory(entry: HistoryEntry) {
  historyOpen.value = false
  view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: entry.sql } })
  view?.focus()
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col" data-testid="sql-editor">
    <div class="flex items-center gap-2 border-b px-2 py-1">
      <Button size="sm" variant="ghost" data-testid="run-query" :disabled="running" @click="runQuery">
        <Play />
        {{ hasSelection ? 'Run selection' : 'Run' }}
      </Button>
      <Button
        v-if="running"
        size="sm"
        variant="ghost"
        class="text-destructive"
        data-testid="cancel-query"
        @click="cancelQuery"
      >
        <OctagonX />
        Cancel
      </Button>
      <span class="flex-1" />
      <span v-if="result" class="font-mono text-[11px] text-muted-foreground" data-testid="query-timing">
        {{ rowsAffected }} row{{ rowsAffected === 1 ? '' : 's' }} · {{ (result.durationMs ?? 0).toFixed(0) }}ms
      </span>
      <ExportMenu
        v-if="activeResult && activeResult.columns.length > 0"
        @copy="copyResult"
        @save="saveResult"
      />
      <Button size="sm" variant="ghost" data-testid="query-history" @click="historyOpen = true">
        <History />
        History
      </Button>
    </div>

    <div ref="host" class="min-h-0 flex-1 overflow-hidden" data-testid="sql-input" />

    <div
      v-if="error || result"
      class="flex min-h-0 flex-[1.2] flex-col border-t"
      data-testid="sql-results"
    >
      <div v-if="result && result.notices.length > 0" class="border-b px-3 py-1.5">
        <p
          v-for="(notice, index) in result.notices"
          :key="index"
          class="font-mono text-[11px] text-muted-foreground"
        >
          <span class="text-amber-500">{{ notice.severity }}</span> {{ notice.message }}
        </p>
      </div>

      <p v-if="error" class="overflow-auto px-3 py-2 font-mono text-xs text-destructive" data-testid="sql-error">
        {{ error }}
      </p>

      <template v-else-if="result">
        <ResultsTable
          v-if="result.statements.length === 1"
          :statement="result.statements[0]"
        />
        <Tabs
          v-else-if="result.statements.length > 1"
          v-model="activeStatement"
          class="flex min-h-0 flex-1 flex-col gap-0"
        >
          <TabsList class="m-1 self-start">
            <TabsTrigger
              v-for="(_, index) in result.statements"
              :key="index"
              :value="String(index)"
              class="font-mono text-xs"
            >
              {{ index + 1 }}
            </TabsTrigger>
          </TabsList>
          <TabsContent
            v-for="(statement, index) in result.statements"
            :key="index"
            :value="String(index)"
            class="flex min-h-0 flex-1 flex-col"
          >
            <ResultsTable :statement="statement" />
          </TabsContent>
        </Tabs>
        <p v-else class="px-3 py-2 font-mono text-xs text-muted-foreground">
          done, no result set
        </p>
      </template>
    </div>

    <CommandDialog v-model:open="historyOpen">
      <CommandInput placeholder="Search query history…" data-testid="history-search" />
      <CommandList data-testid="history-list">
        <CommandEmpty>No queries yet.</CommandEmpty>
        <CommandGroup>
          <CommandItem
            v-for="(entry, index) in historyEntries"
            :key="`${entry.at}-${index}`"
            :value="`${entry.sql} ${index}`"
            data-testid="history-item"
            @select="loadFromHistory(entry)"
          >
            <!-- flex-1 (not ml-auto on the time): CommandItem's trailing check icon also claims ml-auto. -->
            <span class="min-w-0 flex-1 truncate font-mono text-xs" :class="entry.ok ? '' : 'text-destructive'">
              {{ entry.sql }}
            </span>
            <span class="shrink-0 font-mono text-[10px] tabular-nums text-muted-foreground">
              {{ formatTime(entry.at) }}
            </span>
          </CommandItem>
        </CommandGroup>
      </CommandList>
    </CommandDialog>
  </div>
</template>
