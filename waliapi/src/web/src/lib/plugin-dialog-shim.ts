/**
 * `@tauri-apps/plugin-dialog` Web 替身。
 * 文件/目录选择在浏览器中无路径概念，返回 null（调用方按取消处理）；
 * 需要文件内容的页面走 lib/web.ts 的 pickFileAsText 分支。
 */
export interface OpenDialogOptions {
  title?: string;
  filters?: { name: string; extensions: string[] }[];
  multiple?: boolean;
  directory?: boolean;
  defaultPath?: string;
}

export interface SaveDialogOptions {
  title?: string;
  filters?: { name: string; extensions: string[] }[];
  defaultPath?: string;
}

export async function open(
  options: OpenDialogOptions & { multiple: true },
): Promise<string[] | null>;
export async function open(options?: OpenDialogOptions): Promise<string | null>;
export async function open(
  _options?: OpenDialogOptions,
): Promise<string | string[] | null> {
  return null;
}

export async function save(_options?: SaveDialogOptions): Promise<string | null> {
  return null;
}
