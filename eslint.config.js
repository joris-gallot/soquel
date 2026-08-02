import antfu from '@antfu/eslint-config'

export default antfu({
  vue: true,
  typescript: true,
  ignores: [
    '**/dist/**',
    '**/node_modules/**',
    'src-tauri/target/**',
    'src-tauri/gen/**',
    'packages/app/src/lib/bindings.ts',
    // Own workspace, own lockfile, own lint and CI job.
    'landing/**',
  ],
})
