import antfu from '@antfu/eslint-config'

export default antfu({
  vue: true,
  typescript: true,
  ignores: [
    '**/dist/**',
    '**/node_modules/**',
    'src-tauri/target/**',
    'src-tauri/gen/**',
  ],
})
