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
export type DeviceState = "online" | "unauthorized" | "offline";

/** How HermesGate reaches a device. */
export type Transport = "usb" | "wifi";

/** One way to reach a device: the transport and the ADB serial that addresses
 * it. Starting mirroring uses an endpoint serial, so the transport the user
 * picked is the one that reaches `adb`. */
export interface DeviceEndpoint {
  transport: Transport;
  /** `bc058b2c` over USB, `192.168.1.42:37000` over Wi-Fi. */
  serial: string;
}

/**
 * One *physical* device as reported by the Rust core.
 *
 * `adb` exposes the same phone under several serials at once (its hardware
 * serial, the mDNS alias of that USB link, `ip:port` over wireless); the core
 * groups them into one entry, addressed by the most stable serial and listing
 * every transport it is reachable on. One device, one card.
 */
export interface DeviceInfo {
  /** Stable address of the card, not necessarily a usable `adb -s` serial. */
  serial: string;
  state: DeviceState;
  /** Every live transport, USB first, one entry each; a phone that is plugged
   * in *and* paired reports `usb` and `wifi`. Empty only while a device is not
   * attached at all — such a device is no longer reported. */
  endpoints: DeviceEndpoint[];
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
 * Pipeline counters of the running session, sampled once per second by the
 * native video layer. Every field measures a stage of the local pipeline —
 * none of them is input-to-mirror latency, which needs input injection
 * (Phase 3) and is deliberately not measured. The same counters feed the
 * opt-in CSV dump (`HERMESGATE_VIDEO_STATS`).
 */
export interface VideoStats {
  /** Frames decoded in the last sample window, per second. */
  fps: number;
  /** H.264 bytes read off the device in the last window, per second. */
  bytesPerS: number;
  /** Decoder time (decode + colour convert) spent in the last window, in ms. */
  decodeMs: number;
  /** Presentation (blit) time spent in the last window, in ms. */
  blitMs: number;
  /**
   * Frames handed to the presenter in the last window that a newer frame
   * superseded before they were painted (the CSV's `frames_overwritten`).
   * Not a total dropped-frame count.
   */
  overwritten: number;
  /** Frames decoded since the session started. */
  framesDecoded: number;
}

/**
 * Live state of the mirroring session. `phase: "running"` means the engine
 * has presented its first frame; `width`/`height` stay at 0 until then.
 * `reason` carries the failure text when `phase` is `failed`.
 * `stats` carries the 1 Hz pipeline counters while a session is publishing
 * them, and is `null` otherwise — a finished session's numbers are retracted
 * rather than shown as live.
 */
/** What the audio engine measured in the last second (`core::audio`). */
export interface AudioStats {
  /** Render device the session opened, empty when it failed. */
  device: string;
  sampleRate: number;
  channels: number;
  /** PCM bytes read from the device in the last second. */
  kbps: number;
  /** Loudest sample in the last second, `0` means the device is silent. */
  peak: number;
  /** Audio the render device has actually consumed, by its own clock. */
  playedMs: number;
  totalBytes: number;
  /** Why audio is not playing, empty while it is. */
  error: string;
}

export interface MirrorStatus {
  running: boolean;
  phase: "idle" | "starting" | "running" | "stopped" | "failed";
  reason: string;
  width: number;
  height: number;
  serial: string;
  stats: VideoStats | null;
  /**
   * Audio counters for the running session, or `null` when no audio stream is
   * running. `error` is set when the desktop has no usable output device; the
   * picture keeps running in that case.
   */
  audio: AudioStats | null;
  /**
   * Consecutive seconds the decoded picture has carried no visible content.
   * `0` while the picture shows anything. This is a measurement of the
   * picture, so the UI explains the empty mirror without naming a cause:
   * protected content and a display that is off look identical here.
   */
  pictureBlankSecs: number;
}

/**
 * Mirroring modes the Device page offers. They map onto the engine's scrcpy
 * knobs (`core::mirroring::MirrorQuality::preset`): `sharp` is the shipped
 * default (native resolution, 8 Mbit/s, uncapped fps), the others trade pixels
 * for bandwidth. The mode is chosen before Start and applies to the session
 * that start opens.
 */
export type MirrorMode = "sharp" | "balanced" | "smooth";

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
  /** `serial` is the endpoint of the transport to mirror over
   * (`DeviceEndpoint.serial`), not the card's stable address. */
  start: (serial: string, mode: MirrorMode = "sharp") =>
    invoke<void>("start_mirror", { serial, mode }),
  stop: () => invoke<void>("stop_mirror"),
  status: () => invoke<MirrorStatus>("mirror_status"),
  // Navigation keys the device's own buttons send; the panel offers them while
  // a session is running, and the command reports when no session is attached.
  key: (key: "back" | "home" | "recents") => invoke<void>("mirror_key", { key }),
  onChanged: (handler: (status: MirrorStatus) => void) =>
    listen<MirrorStatus>("mirror-changed", (event) => handler(event.payload)),
};

/**
 * One QR pairing ticket minted by the host: the string the phone's camera
 * reads, the mDNS name the phone publishes while it waits, and the QR matrix
 * (flat row-major `width × width`, `true` = dark) the panel draws.
 */
export type WifiQrTicket = {
  payload: string;
  service: string;
  width: number;
  modules: boolean[];
};

/** What a finished QR pairing ended up doing. */
export type QrPairingOutcome = {
  /** `ip:port` the phone answered the pairing on. */
  paired: string;
  /** `ip:port` that was connected afterwards, when it was announced in time. */
  connected: string | null;
};

/**
 * Wireless ADB pairing and connections. `pair` takes the `ip:port` shown on
 * the device's wireless-debugging screen plus the six-digit pairing key;
 * `connect`/`disconnect` drive endpoints that are already paired. Every call
 * rejects with a plain message (for example when no ADB runtime exists).
 *
 * QR pairing is the same flow with the phone as the scanner: `qrStart` mints a
 * ticket to display, `qrAwait` waits for the phone that scanned it and pairs.
 * The ticket is single use — call `qrStart` again for another attempt.
 */
export const wireless = {
  pair: (address: string, key: string) =>
    invoke<void>("wifi_pair", { address, key }),
  connect: (address: string) => invoke<void>("wifi_connect", { address }),
  disconnect: (address: string) =>
    invoke<void>("wifi_disconnect", { address }),
  qrStart: () => invoke<WifiQrTicket>("wifi_qr_start"),
  qrAwait: (timeoutSecs?: number) =>
    invoke<QrPairingOutcome>("wifi_qr_await", { timeoutSecs }),
};
