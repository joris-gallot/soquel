<script setup lang="ts">
import type { IndexInfo } from '@/lib/bindings'
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string, db: string | null, collection: string | null }>()

const indexes = ref<IndexInfo[]>([])
const loadedOnce = ref(false)

let loadSeq = 0

async function load() {
  const seq = ++loadSeq
  if (props.db === null || props.collection === null) {
    indexes.value = []
    return
  }
  try {
    const listed = unwrap(await commands.docIndexes(props.connectionId, props.db, props.collection))
    if (seq !== loadSeq)
      return
    indexes.value = listed
    loadedOnce.value = true
  }
  catch (error) {
    if (seq === loadSeq)
      toast.error(error instanceof Error ? error.message : String(error))
  }
}

watch(() => [props.connectionId, props.db, props.collection], load, { immediate: true })

defineExpose({ refresh: load })
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <div v-if="collection === null" class="flex flex-1 items-center justify-center font-mono text-sm text-muted-foreground">
      <p>soquel=#<span class="ml-1 text-muted-foreground/60">select a collection</span></p>
    </div>
    <ScrollArea v-else class="min-h-0 flex-1">
      <table class="w-full font-mono text-xs">
        <tbody>
          <tr
            v-for="index in indexes"
            :key="index.name"
            class="border-b border-border/40"
            data-testid="index-row"
          >
            <td class="w-56 truncate px-3 py-1.5">
              {{ index.name }}
            </td>
            <td class="px-3 py-1.5 break-all text-muted-foreground">
              {{ index.definition }}
            </td>
            <td class="w-16 px-3 py-1.5 text-right">
              <Badge
                v-if="index.unique"
                variant="outline"
                class="border-transparent bg-amber-500/10 font-mono text-[10px] text-amber-500"
                data-testid="index-unique"
              >
                unique
              </Badge>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-if="loadedOnce && indexes.length === 0" class="px-3 py-2 font-mono text-[11px] text-muted-foreground">
        no indexes
      </p>
    </ScrollArea>
  </div>
</template>
