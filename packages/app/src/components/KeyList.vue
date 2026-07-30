<script setup lang="ts">
import type { KeyEntry } from '@/lib/bindings'
import { watchDebounced } from '@vueuse/core'
import { onMounted, ref } from 'vue'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { ScrollArea } from '@/components/ui/scroll-area'
import { commands } from '@/lib/bindings'
import { containsPattern, formatTtl, KEY_KIND_BADGE } from '@/lib/kv'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string, selected: string | null }>()
const emit = defineEmits<{ select: [key: string] }>()

const SCAN_COUNT = 200

const pattern = ref('')
const glob = ref(false)
const keys = ref<KeyEntry[]>([])
const cursor = ref<string | null>(null)
const scanning = ref(false)
const scannedOnce = ref(false)

function toggleGlob() {
  glob.value = !glob.value
  scan(true)
}

// Latest-wins: a newer scan supersedes in-flight responses instead of blocking.
let scanSeq = 0

async function scan(reset: boolean) {
  const seq = ++scanSeq
  scanning.value = true
  try {
    const page = unwrap(await commands.scanKeys(
      props.connectionId,
      glob.value ? pattern.value : containsPattern(pattern.value),
      reset ? null : cursor.value,
      SCAN_COUNT,
    ))
    if (seq !== scanSeq)
      return
    keys.value = reset ? page.keys : [...keys.value, ...page.keys]
    cursor.value = page.cursor
    scannedOnce.value = true
  }
  catch (error) {
    if (seq === scanSeq)
      toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    if (seq === scanSeq)
      scanning.value = false
  }
}

watchDebounced(pattern, () => scan(true), { debounce: 250 })

/// Fresh scan with the current pattern; parents call this after writes.
function refresh() {
  scan(true)
}

defineExpose({ refresh })
onMounted(() => scan(true))
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="px-2 py-2">
      <InputGroup class="h-7">
        <InputGroupInput
          v-model="pattern"
          :placeholder="glob ? 'match pattern (*)' : 'search keys'"
          class="font-mono text-xs"
          data-testid="key-pattern"
          @keydown.enter="scan(true)"
        />
        <InputGroupAddon align="inline-end">
          <InputGroupButton
            class="font-mono text-[10px]"
            :class="glob && 'bg-accent text-accent-foreground'"
            :aria-pressed="glob"
            title="raw MATCH glob pattern"
            data-testid="key-glob"
            @click="toggleGlob"
          >
            glob
          </InputGroupButton>
        </InputGroupAddon>
      </InputGroup>
    </div>
    <ScrollArea class="min-h-0 flex-1">
      <button
        v-for="entry in keys"
        :key="entry.key"
        type="button"
        class="flex w-full cursor-pointer items-center gap-1.5 px-2 py-1 text-left font-mono text-xs hover:bg-accent/50"
        :class="entry.key === selected && 'bg-accent text-accent-foreground'"
        :data-testid="`key-${entry.key}`"
        @click="emit('select', entry.key)"
      >
        <span
          class="w-9 shrink-0 rounded px-1 text-center text-[10px]"
          :class="KEY_KIND_BADGE[entry.kind].classes"
        >{{ KEY_KIND_BADGE[entry.kind].short }}</span>
        <span class="min-w-0 flex-1 truncate">{{ entry.key }}</span>
        <span v-if="entry.ttlMs !== null" class="shrink-0 text-[10px] text-muted-foreground">
          {{ formatTtl(entry.ttlMs) }}
        </span>
      </button>
      <div class="px-2 py-2">
        <Button
          v-if="cursor !== null"
          size="sm"
          variant="ghost"
          class="h-6 w-full text-[11px]"
          data-testid="scan-more"
          :disabled="scanning"
          @click="scan(false)"
        >
          {{ scanning ? 'scanning…' : 'scan more' }}
        </Button>
        <p v-else-if="scannedOnce" class="font-mono text-[11px] text-muted-foreground" data-testid="key-count">
          {{ keys.length }} key{{ keys.length === 1 ? '' : 's' }}
        </p>
      </div>
    </ScrollArea>
  </div>
</template>
