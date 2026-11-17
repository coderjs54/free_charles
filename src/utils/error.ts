/** 去掉 Tauri / 插件包装后的错误前缀，展示更友好的错误信息 */
export function errorMessage(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error)
  return raw.replace(/^Error invoking remote method '[^']+':\s*(Error:\s*)?/, '').trim()
}
