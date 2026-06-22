# Cosmic Cast Ext

A COSMIC Panel applet for casting the desktop to Miracast wireless displays,
including Microsoft Wireless Display Adapter devices.

## Status

This applet provides the COSMIC panel experience: status checks, a wireless
display popup, launch controls, refresh, direct daemon-backed stream controls
for discovered Miracast displays, and a stop action.

The actual Miracast transport is delegated to
[GNOME Network Displays][gnome-network-displays]. The applet prefers the system
package from apt when `/usr/bin/gnome-network-displays` is installed, and only
falls back to the Flatpak application `org.gnome.NetworkDisplays` when no system
binary is available. Native Miracast source support requires Wi-Fi Direct,
PipeWire/GStreamer capture, RTSP/RTP negotiation, and privileged network
management, so the backend is isolated in `src/cast.rs` for later replacement
with a native implementation.

See [Native Backend Notes](./docs/native-backend.md) for the staged plan and
the GNOME Network Displays source map.

## Runtime Requirements

- NetworkManager running on the host
- A Wi-Fi adapter with Wi-Fi Direct / P2P support
- GNOME Network Displays installed from apt:

```sh
sudo apt install gnome-network-displays
```

Optional, after apt is installed and working:

```sh
flatpak uninstall org.gnome.NetworkDisplays
```

Keep the Flatpak installed if the apt package can launch the UI but does not
ship `gnome-network-displays-daemon`; the applet can use the system picker and
the Flatpak daemon together.

On Pop!_OS/Ubuntu noble, the apt package is currently `0.92.1-2build2`. That
build can launch the GNOME Network Displays UI, but it does not include the
`gnome-network-displays-daemon` binary used by this applet's Miracast-only DBus
filter. In that case the applet still opens the apt version, but the
**Miracast Displays** row reports that daemon-backed scanning is unavailable.

## GNOME Network Displays Daemon

`gnome-network-displays-daemon` is not available from the current Pop!_OS/Ubuntu
noble apt package. To get the daemon, use one of these options:

- Keep the Flathub build installed. Version `0.99.0` includes
  `/app/bin/gnome-network-displays-daemon`.
- Build a newer GNOME Network Displays release from source and install it under
  `/usr/local` or package it for the system.

The applet still prefers `/usr/bin/gnome-network-displays` for opening the
casting UI when the apt package is installed. If the system package does not
ship `gnome-network-displays-daemon` but the Flathub build is installed, the
applet falls back to the Flatpak daemon for the protocol-tagged Miracast-only
display scan and direct stream start/stop controls.

There is not a separate daemon repository in this checkout. The daemon lives in
GNOME Network Displays itself:

- Local daemon source: `gnome-network-displays/src/daemon`
- Local stream source: `gnome-network-displays/src/stream`
- Miracast/Wi-Fi Display source: `gnome-network-displays/src/wfd`
- Upstream remote in this checkout:
  `https://gitlab.gnome.org/GNOME/gnome-network-displays.git`

After installing this applet, add **Cosmic Cast Ext** to the COSMIC Panel. Open
the applet and press **Open** to show the wireless display picker, then select
the Windows wireless display adapter from GNOME Network Displays.

## Troubleshooting

If GNOME Network Displays only shows Chromecast devices, the local-network cast
path is working but Miracast discovery is not. Chromecast and Windows Wireless
Display adapters use different protocols: Chromecast devices are discovered on
the regular LAN, while Windows Wireless Display adapters require Miracast over
Wi-Fi Direct/P2P. Chromecast desktop mirroring can also have substantial latency
and buffering, so this applet treats Chromecast devices as irrelevant for the
Windows wireless display workflow.

Use the applet's **Wi-Fi Direct** row as the first signal. It should report
`Available` before Microsoft Wireless Display Adapter devices are expected to
appear. The **Miracast Displays** row is populated from the GNOME Network
Displays daemon, waits briefly for P2P discovery, and filters out Chromecast
devices. If no Miracast displays are shown:

- Put the Windows wireless display adapter into pairing/screen-mirroring mode.
- Make sure Wi-Fi is enabled on the computer running COSMIC.
- Check that NetworkManager exposes a Wi-Fi P2P device with
  `nmcli -t -f DEVICE,TYPE,STATE device status`.
- Install `iw` and check whether the adapter advertises `P2P-client` or
  `P2P-GO` in `iw list`.
- Run GNOME Network Displays with debug logging and look for
  `WFDP2PProvider: Found a new sink` or `WFDP2PProvider: Ignoring peer`.

For a focused discovery log with the apt package, close GNOME Network Displays,
put the wireless display adapter into pairing mode, then run:

```sh
G_MESSAGES_DEBUG=all gnome-network-displays
```

For the Flatpak package, use:

```sh
flatpak run --command=gnome-network-displays-daemon \
  --env=G_MESSAGES_DEBUG=all \
  org.gnome.NetworkDisplays
```

Useful log lines:

- `NdNMDeviceRegistry: Creating Wi-Fi P2P provider` means NetworkManager exposed
  the P2P device.
- `WFDP2PProvider: Found a new sink` means the Miracast adapter was discovered.
- `WFDP2PProvider: Ignoring peer ... no WFDIEs set` means a P2P peer was seen,
  but it did not advertise Wi-Fi Display capability.
- If the log only alternates between `Starting P2P discovery`, `Started P2P
  discovery`, and `Restarting P2P discovery`, then GNOME Network Displays is not
  seeing any P2P peer at all. At that point, verify the adapter is actively on
  its Miracast pairing screen, power-cycle it, move it close to the computer,
  and test with a second Wi-Fi adapter if one is available.

On this system, NetworkManager exposes `wifi-p2p` but `iw` is not installed. To
inspect the wireless adapter's advertised P2P modes, install `iw` and run:

```sh
iw list | grep -E 'P2P-client|P2P-GO|P2P-device'
```

## Installation

A [justfile](./justfile) is included by default for the [casey/just][just] command runner.

- `just` builds the application with the default `just build-release` recipe
- `just run` builds and runs the application
- `just install` installs the project into the system
- `just vendor` creates a vendored tarball
- `just build-vendored` compiles with vendored dependencies from that tarball
- `just check` runs clippy on the project to check for linter warnings
- `just check-json` can be used by IDEs that support LSP

## Translators

[Fluent][fluent] is used for localization of the software. Fluent's translation files are found in the [i18n directory](./i18n). New translations may copy the [English (en) localization](./i18n/en) of the project, rename `en` to the desired [ISO 639-1 language code][iso-codes], and then translations can be provided for each [message identifier][fluent-guide]. If no translation is necessary, the message may be omitted.

## Packaging

If packaging for a Linux distribution, vendor dependencies locally with the `vendor` rule, and build with the vendored sources using the `build-vendored` rule. When installing files, use the `rootdir` and `prefix` variables to change installation paths.

```sh
just vendor
just build-vendored
just rootdir=debian/cosmic-cast-ext prefix=/usr install
```

It is recommended to build a source tarball with the vendored dependencies, which can typically be done by running `just vendor` on the host system before it enters the build environment.

## Developers

Developers should install [rustup][rustup] and configure their editor to use [rust-analyzer][rust-analyzer].

[fluent]: https://projectfluent.org/
[fluent-guide]: https://projectfluent.org/fluent/guide/hello.html
[gnome-network-displays]: https://flathub.org/apps/org.gnome.NetworkDisplays
[iso-codes]: https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
[just]: https://github.com/casey/just
[rustup]: https://rustup.rs/
[rust-analyzer]: https://rust-analyzer.github.io/
[sccache]: https://github.com/mozilla/sccache
