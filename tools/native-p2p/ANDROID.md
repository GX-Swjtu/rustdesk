# Android native P2P

Android uses the same default-on `enable-native-p2p` option and signed
`native-udp-punch` capability as Windows/Linux. An unset option or `Y` enables
all five native UDP strategies; `N` disables them. An explicitly saved `N` is
preserved. Both endpoints and hbbs must support the capability.

The application retains its minimum API 22. There is no additional requirement
for an open Activity, Wi-Fi, network validation or a two-second stability wait.
The previous foreground/Wi-Fi experiment imposed these restrictions; they were
not upstream RustDesk requirements and have been removed at the user's request.

Current package checks are in
[ANDROID-REVIEW-FIXES-20261014.md](ANDROID-REVIEW-FIXES-20261014.md).
The [initial default-on checks](ANDROID-DEFAULT-VALIDATION-20261014.md) precede
the registration and compatibility-budget fixes.
The earlier [validation record](ANDROID-VALIDATION-20261014.md) describes the
superseded opt-in package only.

## Use and roll back

1. Install the arm64 debug test APK. Keep the existing APK and configuration
   backup. A different signing key cannot replace an existing installation;
   do not uninstall an app with valuable settings just to bypass that.
2. Configure the existing native-capable test hbbs and its public key. No hbbs
   update or new port is required. Connect by ID as usual; the new strategy is
   enabled by default. The Android settings toggle is **Enable UDP hole punching
   (Native P2P beta)**. It is separate from the upstream UDP-punch option.
3. Keep TUN running. For direct physical-network testing, exclude the RustDesk
   app `com.carriez.flutter_hbb` in the VPN app's per-app settings. A domain
   DIRECT rule is not proof that every random-port probe bypasses a VPN. The
   client itself neither changes VPN settings nor forces a different network.
4. Controlling another device does not require starting screen sharing. Receiving
   control still uses the original sharing service, permissions, password or
   confirmation and Android background-execution rules. This feature adds no
   requirement to keep the RustDesk Activity visible.
5. Turn the native toggle off to restore the original connection flow. No route,
   VPN, screen-sharing or media configuration needs reverting. A prior saved
   test APK can restore the previous binary, subject to its original signing key.

## Runtime scope

- Only the connection-strategy stage changes. The original service lifecycle,
  screen capture, permission prompts, identity/password/confirmation, input,
  clipboard and established-session handling remain in place.
- Optional LAN candidates are queried once per native attempt from connected
  physical Wi-Fi/Ethernet networks through ConnectivityManager/LinkProperties.
  They do not gate the attempt: unavailable LAN information, no Wi-Fi, a VPN
  route, or cellular use does not by itself disable public UDP punching. The
  current strategy engine is IPv4; transport failures use the existing fallback.
- No Activity resume/pause hooks, network callbacks, stability timers, wake
  locks, route changes, process/socket network binding or new Android permissions
  are introduced. MainApplication only initializes the LAN-query JNI references.
- Existing user cancellation still closes pending native work without starting
  a fallback. Turning off native P2P or UDP cancels an unfinished native attempt.
  Backgrounding or a network change is handled by the original lifecycle and
  sockets; this module adds no separate cancellation policy for either event.
- A sharing mediator advertises capability on its existing UDP channel. A
  controller without that mediator advertises one temporary signed capability
  on demand, sending its existing device key only if hbbs requests it through
  the original `RegisterPeer/request_pk` exchange. Registration shares the 45-second
  budget and is withdrawn on failure/cancel or after authenticated KCP
  confirmation. Existing registration addresses are reused when available.
- Mode 0/1/3 use one probe socket; Mode 2/4 may allocate 256 additional sockets.
  Limits remain 257 probe sockets, 1000 random targets, 8192 probe datagrams,
  38 seconds for probes and 45 seconds total, including reserved KCP time. A
  controller may briefly own one extra UDP registration socket.
- Explicit RustDesk proxies, WebSocket signaling, force-relay and disabled UDP
  prevent the Android native path. Unsupported peers/servers use the original
  flow. Temporary registration and server capability probing share one
  one-second compatibility deadline. Only a supported session proceeds to the
  remaining portion of the 45-second total attempt budget.

## PC control

Windows/Linux and Android expose **Enable UDP hole punching (Native P2P beta)**
using the same `enable-native-p2p` option and default-on behavior. On PC it is in
**Settings > General > Other**, beside the upstream punching options;
on Android it is on **Settings**. The switch is shown for a self-hosted server
and respects the existing hidden/fixed-settings policies. It uses the existing
service-synchronized option API without requiring the network page's unlock.
The existing **Enable UDP hole punching** checkbox remains a separate upstream-strategy
option (`enable-udp-punch`).

On an installed Windows test client, from an administrator PowerShell in its
installation directory:

```powershell
.\rustdesk.exe --option enable-native-p2p N  # disable
.\rustdesk.exe --option enable-native-p2p Y  # enable
```

These existing CLI commands write through service IPC. Query with
`.\rustdesk.exe --option enable-native-p2p`; an empty value means default-on.
The CLI retains its upstream administrator check; the graphical checkbox does
not invoke the CLI or need that elevation.
Allow the existing registration refresh to publish the new capability before
another connection. The graphical switch uses the existing service IPC path;
it does not change the media, login, permission or fallback implementation.

## Build and verify

Use the Android toolchain/build commands in README.md. Build the arm64 Rust
release library with the existing `flutter,hwcodec` features, copy it into the
arm64 JNI directory, then assemble the debug APK. No bridge generation, new
Cargo dependency, SDK or permission upgrade is needed. Verify APK signing, ABI,
embedded Rust-library SHA-256 and the `Java_ffi_FFI_initNativeP2p` export.

The seven targeted host tests cover default-on/no-LAN admission, disable/UDP-off,
settings cancellation and socket cleanup, bounded LAN candidates, temporary
registration across rapid reconnects, the shared registration/capability deadline,
original bypass policies and KCP handoff. Host tests and successful
compilation do not establish Android real-device P2P or background operation.

Real-device acceptance remains pending: both connection directions with TUN;
actual authenticated UDP-native picture/input and relay-traffic evidence; old
peers/servers, force-relay and failure fallback; Wi-Fi/cellular/VPN changes;
background/lock-screen behavior under the original service; repeated-attempt
FD/memory cleanup, battery impact and file transfer. No Android device was attached.

## Existing paths touched

| Path | Required change |
|---|---|
| Common Cargo and client/common `lib.rs` gates | Link the existing portable module on Android. |
| `src/client.rs`, `src/client/native_p2p.rs` | Android strategy entry, bounded identity registration, cancel and existing KCP handoff. |
| `src/native_p2p.rs` | Android settings gate, invitation handling and optional LAN candidates. |
| `src/rendezvous_mediator.rs` | Android capability advertisement and refresh on option changes. |
| `src/ui_session_interface.rs` | Extend the existing user-cancel hook to the Android attempt. |
| `src/ui_interface.rs` | Feature/UDP option changes cancel pending native work and refresh capability. |
| `MainApplication.kt`, `ffi.kt` | Initialize a process-level JNI reference and expose a read-only LAN query. |
| Mobile settings and constants | Default-on toggle using the existing native option. |
| Desktop settings General > Other panel | Same default-on toggle, using the original checkbox and service-synchronized option API without a network-page unlock. |

MainActivity and MainService retain their original implementations. New Android
logic stays under `src/native_p2p/` and in `NativeP2pNetwork.kt`. No strategy,
protobuf, hbbs/hbbr, media, permission or dependency upgrade is included.
