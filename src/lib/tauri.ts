import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  /** Module lifecycle: `planned`, `partial` or `ready`. */
  state: string;
  responsibility: string;
}

/** Lifecycle of a known device. */
export type DeviceState = "online" | "unauthorized" | "offline" | "disconnected";

/** One Android device as reported by the Rust core. */
export interface DeviceInfo {
  serial: string;
  state: DeviceState;
  transport: "usb" | "wifi";
  manufacturer: string | null;
  model: string | null;
  androidVersion: string | null;
}

/** Availability of the ADB runtime, reported with every device list. */
export interface AdbStatus {
  available: boolean;
  source: string | null;
  path: string | null;
  error: string | null;
}

/** Result of one device refresh. */
export interface DeviceSnapshot {
  devices: DeviceInfo[];
  adb: AdbStatus;
}

/**
 * Live state of the mirroring session. `phase: "running"` means the engine
 * has presented its first frame; `width`/`height` stay at 0 until then.
 * `reason` carries the failure text when `phase` is `failed`.
 */
export interface MirrorStatus {
  running: boolean;
  phase: "idle" | "starting" | "running" | "stopped" | "failed";
  reason: string;
  width: number;
  height: number;
  serial: string;
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

/**
 * Device listing lives entirely on the native side: `list` asks the Rust core
 * for a snapshot, `onChanged` streams the `devices-changed` event it emits
 * when a device is attached, detached or changes state.
 */
export const devices = {
  list: () => invoke<DeviceSnapshot>("list_devices"),
  onChanged: (handler: (snapshot: DeviceSnapshot) => void) =>
    listen<DeviceSnapshot>("devices-changed", (event) => handler(event.payload)),
};

/**
 * Mirroring lives entirely on the native side: `start` and `stop` drive the
 * engine for one device serial, `status` reads the current session on demand,
 * and `onChanged` streams the `mirror-changed` event the engine emits on
 * every transition. `start`/`stop` reject with a plain message on failure.
 */
export const mirror = {
  start: (serial: string) => invoke<void>("start_mirror", { serial }),
  stop: () => invoke<void>("stop_mirror"),
  status: () => invoke<MirrorStatus>("mirror_status"),
  onChanged: (handler: (status: MirrorStatus) => void) =>
    listen<MirrorStatus>("mirror-changed", (event) => handler(event.payload)),
};
