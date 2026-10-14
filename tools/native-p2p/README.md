# Native UDP punching: testing and maintenance

This fork adds a bounded, authenticated UDP Mode 0/1/2/3/4 attempt before the existing
RustDesk connection flow on Windows/Linux. The feature is enabled unless the
`enable-native-p2p` option is `N`. Android uses the same default-on switch,
described in [ANDROID.md](ANDROID.md); iOS/macOS compile out the hooks.
Explicit RustDesk proxies, WebSocket signaling and forced relay keep their
existing behavior. The transport still uses the upstream KCP, device identity,
login/password/confirmation, clipboard, input and file-transfer implementation.

The **Enable UDP hole punching (Native P2P beta)** graphical switch is in PC
**Settings > General > Other** and Android **Settings** when using a self-hosted server.
Both use `enable-native-p2p`, default on, and respect an explicit `N`. The original
UDP punching switch remains independent. The PC checkbox uses the existing
service-sync API without a network-page unlock and respects hidden/fixed settings;
no new configuration or connection-flow mechanism is added.

The common implementation, protocol details, limits, diagnostic usage and frp
v0.71.0 attribution are in `libs/hbb_common/native-p2p/README.md` and `NOTICE`.
The optional dependency is restricted to Windows/Linux/Android target dependencies in
`hbb_common/Cargo.toml`, in addition to the client `cfg` guards.

The only signed feature declaration is `native-udp-punch` with an enabled flag.
There is no V1/V2 capability split, `strategies` field or V2 option. Both endpoints
and hbbs must support the exact capability; otherwise RustDesk uses its original
flow. Windows/Linux retain the one-second initial server wait. Android allows
up to five seconds for registration and negotiation together on every attempt,
including first contact. Invalid or unsupported responses return immediately;
only silence consumes the full wait. Both deadlines remain inside the 45-second
attempt budget. Old experimental native packages
also fall back. The unchanged protobuf `NativeP2pV1` name is a wire-envelope
identifier, not a second feature declaration.

The five strategy families, LAN candidates and TCP-verified multiple exits share
one 45-second deadline. Selection subtracts one history point; only the existing
KCP/device-identity secure handshake rewards two points. Diagnostic echo results
do not reward history or replace desktop acceptance. The common README contains
the exact roles, limits, source validation and coverage constraints.

## Build

Initialize submodules recursively. Use the existing pinned versions from
`.github/workflows/flutter-build.yml`, `flutter/ndk_arm64.sh` and `vcpkg.json`;
do not update library dependencies to integrate this feature.

The first Windows test package uses Flutter 3.24.5, bridge generator 1.80.1,
MSVC 2022, libclang 15.0.6 and the repository's vcpkg baseline. From the repository:

```powershell
# In the MSVC build environment, with VCPKG_ROOT and LIBCLANG_PATH set:
flutter_rust_bridge_codegen --rust-input ./src/flutter_ffi.rs --dart-output ./flutter/lib/generated_bridge.dart --c-output ./flutter/macos/Runner/bridge_generated.h --llvm-path /path/to/clang-15.0.6
cargo build --locked --release --lib --features flutter,hwcodec
cd flutter
flutter build windows --release --no-pub
```

Run the existing bridge generator before the Flutter build when generated files
are absent. Copy `target/release/librustdesk.dll` into the Flutter Windows Release
directory before packaging that entire directory. Keep all Flutter assets/plugin
DLLs. A Rust DLL alone is not an installable client. Record the package hash and
both parent/submodule revisions. This package has the usual default Rust features
plus `flutter,hwcodec`; it does not require the optional `vram` feature.

For Android arm64, prefer the upstream Linux build host (WSL is sufficient):

```sh
export ANDROID_NDK_HOME=/path/to/android-ndk-r28c
export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android="--sysroot=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot -resource-dir=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/lib/clang/19 -D__ANDROID_API__=21"
export VCPKG_ROOT=/path/to/vcpkg
rustup target add aarch64-linux-android
cargo install cargo-ndk --version 3.1.2 --locked
cargo ndk --platform 21 --target aarch64-linux-android build --locked --release --lib --features flutter,hwcodec
```

Install the existing `arm64-android` vcpkg dependencies first. A host libclang,
C/C++ toolchain, make, Perl and pkg-config are required. The existing hwcodec and
libsodium build scripts contain host-platform checks; Windows cross compilation
of these old dependencies is not the reproducible path used for this validation.
`vcpkg-triplets/arm64-android.cmake` is an optional build-only overlay for building
the Android C libraries with vcpkg on Windows/NDK r28c. It sets the assembler target
without modifying upstream ports.

Copy `liblibrustdesk.so` to
`flutter/android/app/src/main/jniLibs/arm64-v8a/librustdesk.so` and the NDK's
`libc++_shared.so` alongside it. Build an arm64 APK with the repository's Android
SDK/Gradle requirements. Use a test signing key for validation; do not overwrite
the user's installed Android application or production signing credentials.

The validated APK build uses JDK 17 and Android SDK 36. From `flutter/android`,
after bridge generation and `flutter pub get` in `flutter`, run:

```powershell
./gradlew.bat ':app:assembleDebug' '--console=plain' '--max-workers=4' `
  '-Dorg.gradle.jvmargs=-Xmx4096m -Dfile.encoding=UTF-8' `
  '-Pkotlin.incremental=false' '-Pkotlin.compiler.execution.strategy=in-process' `
  '-Ptarget-platform=android-arm64' '-Psplit-per-abi=true'
```

The heap and Kotlin flags are build-host settings: the default 1 GiB heap was
insufficient, and Kotlin incremental compilation could not relativize paths
when the package cache and source were on different Windows drives. No Android
application build file needs changing. The output is
`flutter/build/app/outputs/flutter-apk/app-arm64-v8a-debug.apk`; an external Gradle
build-directory override changes that prefix. Check it with `aapt dump badging`
and `apksigner verify --verbose`, and compare the APK's embedded
`lib/arm64-v8a/librustdesk.so` hash with the compiled library. This is a debug-signed
test package, not a production-signed Android release.

## Temporary Windows deployment and rollback

`Use-TestClient.ps1` requires an administrator PowerShell because it changes the
existing RustDesk service. It backs up user and LocalService configuration,
unpacks the package under Program Files, switches the service executable and
selects the independent test server. Original installed binaries stay in place.
The returned backup directory contains `state.json` and the configuration backup.
Treat that directory as private: configuration can contain saved credentials.

```powershell
./tools/native-p2p/Use-TestClient.ps1 -Package C:\test\client.zip `
  -Server TEST_HOST:22116 -ServerKey TEST_SERVER_PUBLIC_KEY

./tools/native-p2p/Use-TestClient.ps1 -Mode Restore `
  -BackupDirectory C:\path\from\the\Prepare\result
```

For a subsequent test build, run `Update-TestClient.ps1 -Package C:\test\client.zip`
in the same administrator shell. It backs up the installed test package, replaces
only that test directory and restarts its service. It preserves the test server,
configuration and original rollback path. Use `Use-TestClient.ps1 -Mode Restore`
with the first deployment's backup to return to the original installation.

Keep the same interactive desktop session active during acceptance. Upstream
Windows IPC validates both process identity and session identity. A disconnected
RDP session and the active console session are different contexts; do not weaken
IPC authentication to make a test harness work across them.

`Set-MihomoRules.ps1` backs up the selected persistent profile/runtime YAML and
adds narrow process/domain DIRECT rules while retaining TUN. Use its parameters
for the active profile/controller. Check the runtime TUN state, actual STUN
mapping and authenticated peer address: the word DIRECT alone is not evidence.
For the Android experiment, exclude the RustDesk application from the VPN app's
per-app routing while retaining TUN for other applications; see ANDROID.md.

## Required acceptance evidence

The [2026-10-14 on-demand acceptance record](ACCEPTANCE-20261014.md) records the
five modes, candidate validation, single capability, builds and final Windows
tests. The [2026-10-13 v4 record](ACCEPTANCE-20261013.md) remains historical evidence.

Start the receiver and captures before the caller. Keep production hbbs/hbbr and
frp untouched; the test server uses its own directory/database/keys and ports
22115–22119. Diagnostic echo results are separate from actual remote-desktop
acceptance.

- Run the common module tests for signatures, replay, address validation,
  unsupported capability, STUN timeout, cancellation and socket cleanup.
- Confirm Android links `rustdesk-native-p2p`, enables it by default and respects
  the disable switch without additional foreground/Wi-Fi restrictions.
- Record actual `UDP-native` transport and successful identity/login, not only
  a received UDP probe. Collect both endpoint mappings and relay-port capture.
- Ten independent cold remote-desktop connections in each Windows direction;
  at least nine native UDP successes per direction. Record fallbacks as fallbacks.
- Thirty minutes of continuous remote desktop, mouse/keyboard, clipboard and a
  100 MiB RustDesk file transfer with matching hashes.
- Test unsupported/disabled peers, forced relay and an old server separately.
- Report Android Rust compilation, APK build and real-device compatibility as
  three separate results. Missing real-device evidence remains pending.

## Existing paths changed (minimization review)

| Existing file | Necessary change |
| --- | --- |
| `Cargo.toml`, `Cargo.lock` | Enable the optional common module and pin its dependencies. |
| `.gitmodules`, `libs/hbb_common` | Pin the feature branch on the client's own common-library baseline. |
| `src/lib.rs` | Compile the new module on Windows/Linux/Android. |
| `src/client.rs` | One platform/forced-relay-gated attempt before the unchanged legacy flow. |
| `src/rendezvous_mediator.rs` | Signed capability registration and platform-gated invitation dispatch. |
| `src/ui_session_interface.rs` | Cancel the pending attempt when the existing session closes. |
| `libs/base/src/config/keys.rs` | Register the disable option with the existing settings/sync machinery. |

All traversal logic is in the two new client modules and the common crate.
For this expansion, the existing runtime changes are limited to
`src/native_p2p.rs` (the single signed capability), `src/client/native_p2p.rs`
(the unversioned transport label), the common pin and target-specific adapter
query dependencies. `Set-MihomoRules.ps1` also includes the KCP diagnostic process.
No additional upstream runtime hook is needed.
The Android integration adds isolated on-demand LAN discovery, a JNI hook and
mobile toggle listed in [ANDROID.md](ANDROID.md). No media, KCP implementation,
TCP/WebRTC algorithm, Android permission or foreground service is changed. Keep the client's `hbb_common`
baseline separate from the server baseline and cherry-pick only P2P commits;
do not merge the whole common-library branch across the two parents.

The native path enters `KcpStream::connect/accept` directly after authenticated
UDP probing. It deliberately skips the legacy UDP probe exchange: a delayed
native response is a nonempty packet that the legacy listener interprets as the
first KCP packet, so it can stop answering the caller's legacy probes too early.
The old UDP path is unchanged. The focused `native_kcp_ignores_late_probe_replies`
test covers repeated native answers arriving before a HardNAT caller starts KCP.

## Isolated NAT acceptance

The diagnostic reuses the production common module and `src/kcp_stream.rs`:

```sh
cargo build --locked --release --manifest-path tools/native-p2p/diagnostic/Cargo.toml
sudo python3 tools/native-p2p/nat-lab.py --hbbs /path/to/test/hbbs --probe /path/to/native-kcp-probe --output /path/to/new/results --rounds 10 --scenarios direct,predict,birthday,both-predict,mixed,multi,loss,lan,public,blocked,unverified,both-random,cancel,bad-proof
```

Linux `ip`, `nft`, `tc` and `ss` are required. All interfaces, firewall tables,
processes, databases and keys live in uniquely named disposable namespaces.
Keep binaries immutable during a run; results include their SHA-256 hashes.
Retain failed groups and record fixes before another group. LAN/public tests use
explicit veth fixtures; shipped RustDesk always discovers physical interfaces.
Do not label simulation as real dual-HardNAT, multi-egress or Android acceptance.

Rollback of the whole feature uses `enable-native-p2p=N`, synchronised through
the existing Windows service configuration mechanism. There is no partial V2
rollback. To restore the previous test package, run `Update-TestClient.ps1 -Package`
with its saved `previous-client.zip`; the original-installation restore is unchanged.

When testing fallback, note that upstream `--relay` persists the peer's
`force-always-relay` option. Back up and clear that option before another native
test. After changing the service feature option, wait for fresh signed
advertisements from both endpoints. Begin a soak clock only after the native
secure KCP handshake and desktop login are confirmed.
