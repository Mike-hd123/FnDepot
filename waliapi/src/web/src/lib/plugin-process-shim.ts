/** `@tauri-apps/plugin-process` Web 替身：重启等价于刷新页面。 */
export async function relaunch(): Promise<void> {
  location.reload();
}
