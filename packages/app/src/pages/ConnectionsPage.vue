<script setup lang="ts">
import type { ConnectionProfile } from '@/lib/bindings'
import { MoreHorizontal, Plug, Plus, Unplug } from '@lucide/vue'
import { onMounted, ref } from 'vue'
import { toast } from 'vue-sonner'
import ConnectionFormDialog from '@/components/ConnectionFormDialog.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useConnections } from '@/composables/useConnections'
import { ENV_BADGE_CLASSES } from '@/lib/connections'

const { connections, activeIds, refresh, remove, connect, disconnect } = useConnections()

const formOpen = ref(false)
const editing = ref<ConnectionProfile | null>(null)
const busyId = ref<string | null>(null)

onMounted(refresh)

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
    if (activeIds.value.has(profile.id))
      await disconnect(profile.id)
    else
      await connect(profile.id)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    busyId.value = null
  }
}

async function removeProfile(profile: ConnectionProfile) {
  try {
    await remove(profile.id)
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
      <Button size="sm" data-testid="new-connection" @click="openCreate">
        <Plus />
        New connection
      </Button>
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
      <Button size="sm" variant="secondary" @click="openCreate">
        <Plus />
        Add connection
      </Button>
    </div>

    <ul v-else class="divide-y rounded-lg border">
      <li
        v-for="profile in connections"
        :key="profile.id"
        data-testid="connection-row"
        class="flex items-center gap-3 px-4 py-3"
      >
        <span
          class="size-2 shrink-0 rounded-full"
          :class="activeIds.has(profile.id) ? 'bg-[oklch(0.72_0.11_240)]' : 'bg-muted-foreground/30'"
          :data-testid="activeIds.has(profile.id) ? 'status-connected' : 'status-disconnected'"
        />
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="truncate text-sm font-medium">{{ profile.name }}</span>
            <Badge variant="outline" class="font-mono text-[10px]" :class="ENV_BADGE_CLASSES[profile.env]">
              {{ profile.env }}
            </Badge>
          </div>
          <p class="truncate font-mono text-xs text-muted-foreground">
            {{ profile.kind }}://{{ profile.user }}@{{ profile.host }}:{{ profile.port }}/{{ profile.database }}
          </p>
        </div>
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
            <DropdownMenuSeparator />
            <DropdownMenuItem data-testid="row-delete" variant="destructive" @click="removeProfile(profile)">
              Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </li>
    </ul>

    <ConnectionFormDialog v-model:open="formOpen" :profile="editing" />
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
