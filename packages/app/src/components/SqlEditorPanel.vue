<script setup lang="ts">
import type { ConnectorKind, ExportFormat, QueryResult, SchemaSnapshot } from '@/lib/bindings'
import type { HistoryEntry } from '@/lib/query-history'
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { PostgreSQL, sql } from '@codemirror/lang-sql'
import { Compartment, EditorState } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, placeholder } from '@codemirror/view'
import { ChevronDown, History, OctagonX, Play } from '@lucide/vue'
import { useClipboard, useLocalStorage } from '@vueuse/core'
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { toast } from 'vue-sonner'
import ExplainTree from '@/components/ExplainTree.vue'
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useSqlSessions } from '@/composables/useSqlSessions'
import { commands } from '@/lib/bindings'
import { soquelEditorTheme } from '@/lib/codemirror'
import { explainSql, explainTreeText, parseExplain } from '@/lib/explain'
import { EXPORT_FORMATS, pickExportPath } from '@/lib/export'
import { pushHistory } from '@/lib/query-history'
import { unwrap } from '@/lib/result'
import { DEFAULT_SCHEMA, snapshotToNamespace } from '@/lib/sql-schema'

const props = defineProps<{ connectionId: string, kind: ConnectorKind, tabId: string, snapshot?: SchemaSnapshot | null }>()

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

const explainPlans = computed(() =>
  (result.value?.statements ?? []).map(statement => parseExplain(statement)),
)

// mysql EXPLAIN ANALYZE output: an indented TREE, unreadable as a table cell.
const explainTexts = computed(() =>
  (result.value?.statements ?? []).map(statement => explainTreeText(statement)),
)

function explainRaw(index: number): string {
  const statement = result.value?.statements[index]
  return statement ? statement.rows.map(row => row[0] ?? '').join('\n') : ''
}

async function copyResult(format: ExportFormat) {
  const statement = activeResult.value
  if (!statement)
    return
  try {
    const text = unwrap(await commands.formatStatement(statement.columns, statement.rows, format, props.kind, 'results'))
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
    unwrap(await commands.exportStatement(statement.columns, statement.rows, format, props.kind, 'results', path))
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

function runQuery() {
  return execute(currentSql().trim())
}

/// EXPLAIN wraps a single statement: use the selection to explain one of many.
function runExplain(analyze: boolean) {
  const statementSql = currentSql().trim().replace(/;\s*$/, '')
  if (statementSql === '')
    return
  return execute(explainSql(props.kind, analyze, statementSql))
}

async function execute(statementSql: string) {
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
      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <Button size="sm" variant="ghost" data-testid="explain-menu" :disabled="running">
            Explain
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" class="min-w-44 font-mono text-xs">
          <DropdownMenuItem class="text-xs" data-testid="explain-plain" @click="runExplain(false)">
            Explain
          </DropdownMenuItem>
          <DropdownMenuItem
            class="flex-col items-start gap-0.5 text-xs"
            data-testid="explain-analyze"
            @click="runExplain(true)"
          >
            <span>Explain analyze</span>
            <span class="text-[10px] text-muted-foreground">runs the query</span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
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

    <ResizablePanelGroup direction="vertical" auto-save-id="soquel-editor" class="min-h-0 flex-1">
      <ResizablePanel id="editor-input" :min-size="15">
        <div ref="host" class="h-full min-h-0 overflow-hidden" data-testid="sql-input" />
      </ResizablePanel>
      <template v-if="error || result">
        <ResizableHandle with-handle />
        <ResizablePanel
          id="editor-results"
          :default-size="55"
          :min-size="15"
          class="flex min-h-0 flex-col"
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
            <template v-if="result.statements.length === 1">
              <ExplainTree
                v-if="explainPlans[0]"
                :plans="explainPlans[0]"
                :raw="explainRaw(0)"
              />
              <pre
                v-else-if="explainTexts[0]"
                class="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs leading-5"
                data-testid="explain-text"
              >{{ explainTexts[0] }}</pre>
              <ResultsTable v-else :statement="result.statements[0]" />
            </template>
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
                <ExplainTree
                  v-if="explainPlans[index]"
                  :plans="explainPlans[index]"
                  :raw="explainRaw(index)"
                />
                <pre
                  v-else-if="explainTexts[index]"
                  class="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs leading-5"
                >{{ explainTexts[index] }}</pre>
                <ResultsTable v-else :statement="statement" />
              </TabsContent>
            </Tabs>
            <p v-else class="px-3 py-2 font-mono text-xs text-muted-foreground">
              done, no result set
            </p>
          </template>
        </ResizablePanel>
      </template>
    </ResizablePanelGroup>

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
