/** `@tauri-apps/plugin-updater` Web 替身：Web 版无应用内更新，始终无新版本。 */
export interface Update {
  version: string;
  downloadAndInstall(): Promise<void>;
}

export async function check(): Promise<Update | null> {
  return null;
}
