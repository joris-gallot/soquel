<script setup lang="ts">
import { nextTick, ref, useTemplateRef } from 'vue'
import { Input } from '@/components/ui/input'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string, db: string | null, collection: string | null }>()
const emit = defineEmits<{ ran: [] }>()

interface ConsoleEntry {
  prompt: string
  source: string
  lines: string[]
  summary: string | null
  ok: boolean
}

const input = ref('')
const running = ref(false)
const history = ref<ConsoleEntry[]>([])
const recalled = ref(-1)
const log = useTemplateRef('log')

async function run() {
  const source = input.value.trim()
  if (source === '' || running.value || props.db === null || props.collection === null)
    return
  const prompt = `${props.db}.${props.collection}`
  running.value = true
  input.value = ''
  recalled.value = -1
  try {
    const result = unwrap(await commands.docRunQuery(props.connectionId, props.db, props.collection, source))
    const parts = [`${result.docs.length} doc${result.docs.length === 1 ? '' : 's'}`]
    if (result.truncated)
      parts.push('truncated')
    if (result.durationMs !== null)
      parts.push(`${Math.max(Math.round(result.durationMs), 1)} ms`)
    history.value.push({ prompt, source, lines: result.docs, summary: parts.join(' · '), ok: true })
    emit('ran')
  }
  catch (error) {
    history.value.push({
      prompt,
      source,
      lines: [error instanceof Error ? error.message : String(error)],
      summary: null,
      ok: false,
    })
  }
  finally {
    running.value = false
    nextTick(() => log.value?.scrollTo({ top: log.value.scrollHeight }))
  }
}

/// Arrow-up recall, newest first; arrow-down walks back toward the blank line.
function recall(direction: 1 | -1) {
  const entries = history.value
  if (entries.length === 0)
    return
  const next = recalled.value + direction
  if (next < 0) {
    recalled.value = -1
    input.value = ''
    return
  }
  if (next >= entries.length)
    return
  recalled.value = next
  input.value = entries[entries.length - 1 - next].source
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col" data-testid="mongo-console">
    <div ref="log" class="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs">
      <p v-if="history.length === 0" class="text-muted-foreground">
        {{ db && collection ? `${db}.${collection}` : 'mongo' }}&gt;<span class="ml-1 text-muted-foreground/60">{{ collection ? 'find filter { … } or pipeline [ … ]' : 'select a collection first' }}</span>
      </p>
      <div v-for="(entry, index) in history" :key="index" class="mb-2">
        <p class="text-muted-foreground">
          {{ entry.prompt }}&gt; <span class="text-foreground">{{ entry.source }}</span>
        </p>
        <pre
          class="whitespace-pre-wrap break-all"
          :class="entry.ok ? '' : 'text-destructive'"
          data-testid="console-reply"
        >{{ entry.lines.join('\n') }}</pre>
        <p v-if="entry.summary" class="text-[10px] text-muted-foreground">
          {{ entry.summary }}
        </p>
      </div>
    </div>
    <div class="border-t px-3 py-2">
      <Input
        v-model="input"
        placeholder="{ &quot;plan&quot;: &quot;pro&quot; } or [{ &quot;$group&quot;: … }]"
        class="h-7 font-mono text-xs"
        data-testid="console-input"
        :disabled="running || collection === null"
        @keydown.enter="run"
        @keydown.up.prevent="recall(1)"
        @keydown.down.prevent="recall(-1)"
      />
    </div>
  </div>
</template>
