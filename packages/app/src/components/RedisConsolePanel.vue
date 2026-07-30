<script setup lang="ts">
import { nextTick, ref, useTemplateRef } from 'vue'
import { Input } from '@/components/ui/input'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string }>()
const emit = defineEmits<{ ran: [command: string] }>()

interface ConsoleEntry {
  command: string
  lines: string[]
  ok: boolean
}

const input = ref('')
const running = ref(false)
const history = ref<ConsoleEntry[]>([])
const recalled = ref(-1)
const log = useTemplateRef('log')

async function run() {
  const command = input.value.trim()
  if (command === '' || running.value)
    return
  running.value = true
  input.value = ''
  recalled.value = -1
  try {
    const lines = unwrap(await commands.kvRunCommand(props.connectionId, command))
    history.value.push({ command, lines, ok: true })
    emit('ran', command)
  }
  catch (error) {
    history.value.push({
      command,
      lines: [error instanceof Error ? error.message : String(error)],
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
  const commands_ = history.value
  if (commands_.length === 0)
    return
  const next = recalled.value + direction
  if (next < 0) {
    recalled.value = -1
    input.value = ''
    return
  }
  if (next >= commands_.length)
    return
  recalled.value = next
  input.value = commands_[commands_.length - 1 - next].command
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col" data-testid="redis-console">
    <div ref="log" class="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs">
      <p v-if="history.length === 0" class="text-muted-foreground">
        redis&gt;<span class="ml-1 text-muted-foreground/60">type a command (SET, GET, KEYS, …)</span>
      </p>
      <div v-for="(entry, index) in history" :key="index" class="mb-2">
        <p class="text-muted-foreground">
          redis&gt; <span class="text-foreground">{{ entry.command }}</span>
        </p>
        <pre
          class="whitespace-pre-wrap"
          :class="entry.ok ? '' : 'text-destructive'"
          data-testid="console-reply"
        >{{ entry.lines.join('\n') }}</pre>
      </div>
    </div>
    <div class="border-t px-3 py-2">
      <Input
        v-model="input"
        placeholder="redis command"
        class="h-7 font-mono text-xs"
        data-testid="console-input"
        :disabled="running"
        @keydown.enter="run"
        @keydown.up.prevent="recall(1)"
        @keydown.down.prevent="recall(-1)"
      />
    </div>
  </div>
</template>
