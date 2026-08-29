/** `@tauri-apps/plugin-opener` Web 替身：新标签页打开链接。 */
export async function openUrl(url: string): Promise<void> {
  window.open(url, "_blank", "noopener,noreferrer");
}
