# Native Backend Notes

This project should move toward a COSMIC-native frontend first, then a native
transport backend. The current Rust code should not copy GNOME Network Displays
C sources directly: the applet is MPL-2.0, while the GNOME Network Displays
source tree is GPL-3.0-or-later. Use the upstream code as a behavior reference
or isolate it as a separate optional process unless the project is relicensed.

## Current bridge

`src/cast.rs` is the backend boundary. It currently:

- checks NetworkManager and Wi-Fi Direct availability;
- starts the GNOME Network Displays daemon when one is available, including the
  Flatpak daemon when the system package only ships the GTK app;
- reads the daemon's `org.gnome.NetworkDisplays.Manager.Displays` DBus
  property;
- filters discovered displays to Miracast protocols;
- starts and stops daemon-backed streams with `StartStream` and `StopStream`;
- still opens the GNOME Network Displays GTK picker as a fallback.

This is the first COSMIC-native milestone: the panel applet can show and control
Miracast sinks without using the GTK frontend, while the GNOME daemon and stream
process still own transport.

## Upstream source map

Use these files in `gnome-network-displays/` as the implementation map:

- `src/daemon/nd-daemon.c`: daemon lifecycle and provider setup.
- `src/nd-manager.c`: DBus facade, display list, `StartStream`, `StopStream`,
  and systemd transient stream unit creation.
- `src/nd-nm-device-registry.c`: NetworkManager device discovery and provider
  creation.
- `src/nd-wfd-p2p-provider.c`: Wi-Fi Direct discovery, P2P peer handling, and
  WFD information element filtering.
- `src/nd-wfd-p2p-sink.c`: Miracast P2P sink state, peer metadata, P2P
  connection setup, and stream URI generation.
- `src/stream/nd-stream.c`: desktop capture through the portal, PipeWire source
  setup, audio source setup, and stream lifecycle.
- `src/wfd/*.c`: Wi-Fi Display RTSP negotiation, codec selection, media factory,
  RTP setup, and keepalive behavior.

## Staged native plan

1. Keep the COSMIC UI native and talk to the daemon over DBus.
2. Add display selection and per-display state to the applet, then use daemon
   `StartStream`/`StopStream` directly.
3. Package or build the daemon/stream pieces independently from the GTK app with
   `-Dbuild_app=false -Dbuild_daemon=true` while the Rust backend is developed.
4. Replace discovery with Rust code that talks to NetworkManager DBus directly,
   starts P2P discovery, tracks `wifi-p2p` devices, and filters peers with WFD
   information elements.
5. Replace connection setup with Rust code for the selected P2P peer, including
   firewall handling, NetworkManager active-connection tracking, and sink state.
6. Replace streaming with Rust/GStreamer code using the desktop screencast
   portal, PipeWire, an RTSP server, WFD negotiation, H.264, and optional AAC.
7. Remove the GNOME daemon dependency once discovery, connection, streaming,
   stop, error reporting, and logs all work from the Rust backend.

## Hardware checks that matter

Before debugging app code, verify the laptop and desktop separately:

- `nmcli -t -f DEVICE,TYPE,STATE device status` should expose a `wifi-p2p`
  device.
- `iw dev` should show a P2P device for the Wi-Fi adapter.
- `wpa_supplicant` must expose `p2p` capability.
- `wpa_supplicant` must support Wi-Fi Display information elements; otherwise
  peers may be visible but ignored as non-WFD devices.

If the laptop discovers the adapter and the desktop does not, treat that as a
hardware or driver capability difference until the NetworkManager, `iw`, and
`wpa_supplicant` checks prove otherwise.
