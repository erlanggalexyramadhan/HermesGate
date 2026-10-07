import { invoke } from "@tauri-apps/api/core";

/** Application and runtime facts reported by the Rust core. */
export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  arch: string;
}

/** One native core module as registered in Rust. */
export interface ModuleInfo {
  id: string;
  name: string;
  /** Bootstrap state, e.g. `"planned"`. */
  state: string;
  responsibility: string;
}

/**
 * Typed wrappers over the Tauri command layer.
 *
 * The webview can only reach the native core through these commands — no
 * shell, filesystem, or device capability is exposed to the frontend.
 */
export const core = {
  appInfo: () => invoke<AppInfo>("get_app_info"),
  coreStatus: () => invoke<ModuleInfo[]>("get_core_status"),
};
