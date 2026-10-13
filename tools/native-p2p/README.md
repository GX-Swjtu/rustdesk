# Native P2P v1 testing and maintenance

This fork adds a bounded, authenticated UDP Mode 2 attempt before the existing
RustDesk connection flow on Windows/Linux. The feature is enabled unless the
`enable-native-p2p` option is `N`. Android/iOS/macOS compile out the client hooks;
an Android configuration containing `enable-native-p2p=Y` cannot enable them.
Explicit RustDesk proxies, WebSocket signaling and forced relay keep their
existing behavior. The transport still uses the upstream KCP, device identity,
login/password/confirmation, clipboard, input and file-transfer implementation.

The common implementation, protocol details, limits, diagnostic usage and frp
v0.71.0 attribution are in `libs/hbb_common/native-p2p/README.md` and `NOTICE`.
The optional dependency is restricted to Windows/Linux target dependencies in
`hbb_common/Cargo.toml`, in addition to the client `cfg` guards.

The admission fix adds the configured hbbs public key to the signed Begin
request, and sends a signed version-0 capability when this strategy is disabled.
Install the updated Windows clients before the updated hbbs. An older native
test client with the updated keyed server falls back to the original connection
flow. No Android update is required for this fix; its native hooks stay excluded.

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
compatibility package, not a production-signed Android release.

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
Android does not need the new strategy's process/STUN rules.

## Required acceptance evidence

Start the receiver and captures before the caller. Keep production hbbs/hbbr and
frp untouched; the test server uses its own directory/database/keys and ports
22115–22119. Diagnostic echo results are separate from actual remote-desktop
acceptance.

- Run the common module tests for signatures, replay, address validation,
  unsupported capability, STUN timeout, cancellation and socket cleanup.
- Confirm Android's target dependency graph excludes `rustdesk-native-p2p`.
- Record actual `UDP-native-v1` transport and successful identity/login, not only
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
| `src/lib.rs` | Compile the new module only on Windows/Linux. |
| `src/client.rs` | One platform/forced-relay-gated attempt before the unchanged legacy flow. |
| `src/rendezvous_mediator.rs` | Signed capability registration and platform-gated invitation dispatch. |
| `src/ui_session_interface.rs` | Cancel the pending attempt when the existing session closes. |
| `libs/base/src/config/keys.rs` | Register the disable option with the existing settings/sync machinery. |

All traversal logic is in the two new client modules and the common crate.
No media, KCP implementation, TCP/WebRTC algorithm, UI, Android permission,
foreground service or JNI code is changed. Keep the client's `hbb_common`
baseline separate from the server baseline and cherry-pick only P2P commits;
do not merge the whole common-library branch across the two parents.

The native path enters `KcpStream::connect/accept` directly after authenticated
UDP probing. It deliberately skips the legacy UDP probe exchange: a delayed
native response is a nonempty packet that the legacy listener interprets as the
first KCP packet, so it can stop answering the caller's legacy probes too early.
The old UDP path is unchanged. The focused `native_kcp_ignores_late_probe_replies`
test covers repeated native answers arriving before a HardNAT caller starts KCP.
