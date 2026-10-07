# ADR 0002: USB and Wi-Fi share one session-level pipeline

- **Status:** Accepted
- **Date:** 2026-10-07

## Context

HermesGate must support two ways of reaching a device: USB (attached through the
local ADB daemon) and Wi-Fi (wireless pairing/discovery). Mirroring, input and
clipboard behaviour is identical regardless of how the device is reached.
Implementing both transports as separate pipelines would duplicate the most
complex part of the system (decode, render, input timing).

## Decision

- Transport knowledge is confined to `src-tauri/src/core/connection/`, which
  exposes `Transport::{Usb, Wifi}` plus `usb` and `wifi` submodules.
- Everything downstream (`mirroring`, `input`, `clipboard`) consumes an opaque
  connection session and never a transport. Transport details must not leak past
  the connection module.
- The mirroring pipeline is therefore defined as
  `Android capture/encode → transport (connection layer) → native decode → GPU
  render → window(s)`, where only the transport stage differs between USB and
  Wi-Fi.

## Consequences

- Mirroring, input and clipboard are implemented once for both transports.
- Adding another transport later touches only the connection module.
- The connection module must define a uniform session abstraction early —
  to be settled together with Phase 1 (`docs/roadmap.md`).
