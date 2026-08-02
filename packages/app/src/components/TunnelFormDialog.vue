<script setup lang="ts">
import type { TunnelInput, TunnelProfile } from '@/lib/bindings'
import type { TunnelFormValues } from '@/lib/tunnels'
import { computed, ref, watch } from 'vue'
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
import { useKeychain } from '@/composables/useKeychain'
import { useTunnels } from '@/composables/useTunnels'
import { commands } from '@/lib/bindings'
import { CREDENTIAL_MODE_HINTS, CREDENTIAL_MODE_LABELS, CREDENTIAL_MODES, defaultCredentialMode } from '@/lib/connections'
import { CommandError, unwrap } from '@/lib/result'
import { SSH_AUTH_HINTS, SSH_AUTH_LABELS, SSH_AUTH_METHODS, SSH_AUTH_NEEDS_SECRET, toTunnelInput, tunnelFormValues, tunnelSchema } from '@/lib/tunnels'
import { zodFieldErrors } from '@/lib/validation'

const props = defineProps<{ tunnel?: TunnelProfile | null }>()
const emit = defineEmits<{ saved: [] }>()
const open = defineModel<boolean>('open', { required: true })

const { create, update, test } = useTunnels()
const { available: keychainAvailable, problem: keychainProblem } = useKeychain()

function emptyValues(): TunnelFormValues {
  const credentialMode = defaultCredentialMode(keychainAvailable.value)
  return { name: '', host: '', port: 22, user: '', method: 'agent', keyPath: '', secret: '', credentialMode, credentialCommand: '' }
}

const values = ref<TunnelFormValues>(emptyValues())
const secretLabel = computed(() => values.value.method === 'key-file' ? 'Key passphrase' : 'Password')
const secretPlaceholder = computed(() => {
  if (values.value.credentialMode === 'prompt')
    return 'not stored'
  if (props.tunnel)
    return 'unchanged'
  return values.value.method === 'key-file' ? 'empty if none' : ''
})

const commandArgv = ref<string[]>([])
const commandProblem = ref<string | null>(null)

// The core owns the splitting rules; previewing them here would drift.
watch(() => [values.value.credentialMode, values.value.credentialCommand] as const, async ([mode, command]) => {
  if (mode !== 'command' || command.trim() === '') {
    commandArgv.value = []
    commandProblem.value = null
    return
  }
  const result = await commands.parseCredentialCommand(command)
  commandArgv.value = result.status === 'ok' ? result.data : []
  commandProblem.value = result.status === 'ok' ? null : result.error.message
})

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
  values.value = props.tunnel ? tunnelFormValues(props.tunnel) : emptyValues()
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

        <template v-if="SSH_AUTH_NEEDS_SECRET[values.method]">
          <div class="space-y-1.5">
            <Label>{{ secretLabel }} from</Label>
            <Select v-model="values.credentialMode">
              <SelectTrigger data-testid="field-tunnel-credential-mode" class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="mode in CREDENTIAL_MODES"
                  :key="mode"
                  :value="mode"
                  :disabled="mode === 'keychain' && !keychainAvailable"
                  :data-testid="`tunnel-credential-mode-${mode}`"
                >
                  {{ CREDENTIAL_MODE_LABELS[mode] }}
                </SelectItem>
              </SelectContent>
            </Select>
            <p v-if="keychainProblem" data-testid="tunnel-keychain-problem" class="text-xs text-amber-600 dark:text-amber-500">
              {{ keychainProblem }}
            </p>
          </div>

          <div class="space-y-1.5">
            <template v-if="values.credentialMode === 'command'">
              <Label for="tunnel-credential-command">Command</Label>
              <Input
                id="tunnel-credential-command"
                v-model="values.credentialCommand"
                data-testid="field-tunnel-credential-command"
                class="font-mono text-xs"
                placeholder="vault-ssh-password --host {host} --user {user}"
              />
              <p v-if="errors.credentialCommand" class="text-xs text-destructive">
                {{ errors.credentialCommand }}
              </p>
              <p v-else-if="commandProblem" data-testid="tunnel-credential-command-problem" class="text-xs text-destructive">
                {{ commandProblem }}
              </p>
              <p v-else-if="commandArgv.length" data-testid="tunnel-credential-command-argv" class="font-mono text-xs text-muted-foreground">
                runs: <span v-for="(arg, index) in commandArgv" :key="index" class="mr-1 rounded bg-muted px-1 py-0.5">{{ arg }}</span>
              </p>
              <p class="text-xs text-muted-foreground">
                No shell: {{ '{host}' }} {{ '{port}' }} {{ '{user}' }} are substituted, pipes and $(...) are not supported.
              </p>
            </template>
            <template v-else>
              <Label for="tunnel-secret">
                {{ values.credentialMode === 'prompt' ? `${secretLabel} (for Test only)` : secretLabel }}
              </Label>
              <Input
                id="tunnel-secret"
                v-model="values.secret"
                data-testid="field-tunnel-secret"
                type="password"
                :placeholder="secretPlaceholder"
              />
              <p class="text-xs text-muted-foreground">
                {{ CREDENTIAL_MODE_HINTS[values.credentialMode] }}
              </p>
            </template>
          </div>
        </template>

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
