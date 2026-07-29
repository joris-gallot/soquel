<script setup lang="ts">
import type { TunnelInput, TunnelProfile } from '@/lib/bindings'
import type { TunnelFormValues } from '@/lib/tunnels'
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import HostKeyTrustPanel from '@/components/HostKeyTrustPanel.vue'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useTunnels } from '@/composables/useTunnels'
import { commands } from '@/lib/bindings'
import { CommandError, unwrap } from '@/lib/result'
import { SSH_AUTH_HINTS, SSH_AUTH_LABELS, SSH_AUTH_METHODS, SSH_AUTH_NEEDS_SECRET, toTunnelInput, tunnelSchema } from '@/lib/tunnels'
import { zodFieldErrors } from '@/lib/validation'

const props = defineProps<{ tunnel?: TunnelProfile | null }>()
const emit = defineEmits<{ saved: [] }>()
const open = defineModel<boolean>('open', { required: true })

const { create, update, test } = useTunnels()

function emptyValues(): TunnelFormValues {
  return { name: '', host: '', port: 22, user: '', method: 'agent', keyPath: '', secret: '' }
}

const values = ref<TunnelFormValues>(emptyValues())
const errors = ref<Record<string, string>>({})
const saving = ref(false)
const testing = ref(false)
const testResult = ref<{ ok: boolean, message: string } | null>(null)
const defaultKeys = ref<string[]>([])

watch(open, async (isOpen) => {
  if (!isOpen)
    return
  errors.value = {}
  testResult.value = null
  defaultKeys.value = unwrap(await commands.defaultSshKeys())
  values.value = props.tunnel
    ? {
        name: props.tunnel.name,
        host: props.tunnel.host,
        port: props.tunnel.port,
        user: props.tunnel.user,
        method: props.tunnel.auth.method,
        keyPath: props.tunnel.auth.method === 'key-file' ? props.tunnel.auth.path : '',
        secret: '',
      }
    : emptyValues()
})

// Prefill the first identity OpenSSH itself would try, once the user asks for a key.
watch(() => values.value.method, (method) => {
  if (method === 'key-file' && values.value.keyPath === '')
    values.value.keyPath = defaultKeys.value[0] ?? ''
})

function parse(): TunnelInput | null {
  const result = tunnelSchema.safeParse(values.value)
  if (!result.success) {
    errors.value = zodFieldErrors(result.error)
    return null
  }
  errors.value = {}
  return toTunnelInput(result.data)
}

async function runTest() {
  const input = parse()
  if (!input)
    return
  testing.value = true
  testResult.value = null
  try {
    await test(input, props.tunnel?.id)
    testResult.value = { ok: true, message: 'Tunnel OK' }
  }
  catch (error) {
    // The host key panel owns this failure mode.
    if (error instanceof CommandError && error.kind === 'host-key-untrusted')
      testResult.value = null
    else
      testResult.value = { ok: false, message: error instanceof Error ? error.message : String(error) }
  }
  finally {
    testing.value = false
  }
}

async function save() {
  const input = parse()
  if (!input)
    return
  saving.value = true
  try {
    if (props.tunnel)
      await update(props.tunnel.id, input)
    else
      await create(input)
    open.value = false
    emit('saved')
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          {{ tunnel ? 'Edit tunnel' : 'New SSH tunnel' }}
        </DialogTitle>
        <DialogDescription>
          {{ tunnel ? 'Update the tunnel details.' : 'A bastion shared by any connection that references it.' }}
        </DialogDescription>
      </DialogHeader>

      <form class="space-y-4" @submit.prevent="save">
        <div class="space-y-1.5">
          <Label for="tunnel-name">Name</Label>
          <Input id="tunnel-name" v-model="values.name" data-testid="field-tunnel-name" placeholder="prod bastion" />
          <p v-if="errors.name" class="text-xs text-destructive">
            {{ errors.name }}
          </p>
        </div>

        <div class="grid grid-cols-[1fr_8rem] gap-3">
          <div class="space-y-1.5">
            <Label for="tunnel-host">Host</Label>
            <Input id="tunnel-host" v-model="values.host" data-testid="field-tunnel-host" class="font-mono" />
            <p v-if="errors.host" class="text-xs text-destructive">
              {{ errors.host }}
            </p>
          </div>
          <div class="space-y-1.5">
            <Label for="tunnel-port">Port</Label>
            <Input id="tunnel-port" v-model="values.port" data-testid="field-tunnel-port" type="number" class="font-mono" />
            <p v-if="errors.port" class="text-xs text-destructive">
              {{ errors.port }}
            </p>
          </div>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <div class="space-y-1.5">
            <Label for="tunnel-user">User</Label>
            <Input id="tunnel-user" v-model="values.user" data-testid="field-tunnel-user" class="font-mono" />
            <p v-if="errors.user" class="text-xs text-destructive">
              {{ errors.user }}
            </p>
          </div>
          <div class="space-y-1.5">
            <Label>Authentication</Label>
            <Select v-model="values.method">
              <SelectTrigger data-testid="field-tunnel-auth" class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="method in SSH_AUTH_METHODS" :key="method" :value="method">
                  {{ SSH_AUTH_LABELS[method] }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <p v-if="SSH_AUTH_HINTS[values.method]" class="text-xs text-muted-foreground">
          {{ SSH_AUTH_HINTS[values.method] }}
        </p>

        <div v-if="values.method === 'key-file'" class="space-y-1.5">
          <Label for="tunnel-key-path">Key file</Label>
          <Input
            id="tunnel-key-path"
            v-model="values.keyPath"
            data-testid="field-tunnel-key-path"
            class="font-mono"
            placeholder="~/.ssh/id_ed25519"
          />
          <p v-if="errors.keyPath" class="text-xs text-destructive">
            {{ errors.keyPath }}
          </p>
          <p v-if="defaultKeys.length === 0" class="text-xs text-muted-foreground">
            No key found in ~/.ssh. Generate one with
            <span class="font-mono">ssh-keygen -t ed25519</span>, or pick another authentication method.
          </p>
          <div v-else-if="defaultKeys.length > 1" class="flex flex-wrap gap-1">
            <button
              v-for="key in defaultKeys"
              :key="key"
              type="button"
              class="rounded border px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground hover:text-foreground"
              @click="values.keyPath = key"
            >
              {{ key }}
            </button>
          </div>
        </div>

        <div v-if="SSH_AUTH_NEEDS_SECRET[values.method]" class="space-y-1.5">
          <Label for="tunnel-secret">{{ values.method === 'key-file' ? 'Key passphrase' : 'Password' }}</Label>
          <Input
            id="tunnel-secret"
            v-model="values.secret"
            data-testid="field-tunnel-secret"
            type="password"
            :placeholder="tunnel ? 'unchanged' : (values.method === 'key-file' ? 'empty if none' : '')"
          />
        </div>

        <HostKeyTrustPanel />

        <p
          v-if="testResult"
          data-testid="tunnel-test-result"
          class="font-mono text-xs"
          :class="testResult.ok ? 'text-emerald-500' : 'text-destructive'"
        >
          {{ testResult.message }}
        </p>

        <DialogFooter class="gap-2 sm:justify-between">
          <Button
            type="button"
            variant="outline"
            data-testid="test-tunnel"
            :disabled="testing"
            @click="runTest"
          >
            {{ testing ? 'Testing…' : 'Test connection' }}
          </Button>
          <Button type="submit" data-testid="save-tunnel" :disabled="saving">
            {{ tunnel ? 'Save changes' : 'Create tunnel' }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
