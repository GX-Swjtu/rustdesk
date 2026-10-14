# Android default-on native P2P validation, 2026-10-14

This package replaces the earlier foreground/Wi-Fi-only experiment at the user's
request. Android now uses the desktop default: only `enable-native-p2p=N` disables
the feature. A previously saved `N` remains respected. No Activity visibility,
Wi-Fi-only, network-validation or stability-delay requirement remains.

The working-tree changes are based on client `bea054765` and client-common
`ce2570ba`. They have not been committed or pushed. Windows packages, both server
repositories, production/test services and TUN configuration were not changed.

## Package

Artifacts are in the workspace's `artifacts/android-native-default-20261014/`.
The APK is debug-signed, arm64-v8a only, version 1.5.1 (2068), min SDK 22 and
target SDK 36. No Android permission, SDK or library dependency was added.

| Artifact | SHA-256 |
|---|---|
| `rustdesk-native-p2p-android-arm64-default-on.apk` | `bc00ec800b694c02caab33d97c662fc31d68babfdc382b55a2900ac2da3a3735` |
| Compiled and APK-embedded Rust library | `89304c81a975bb82f9ad6729e1d0002428faa4ff776e500c330a8da0c00ac40a` |

## Completed checks

- Android arm64 Rust release build: passed, locked/offline existing dependencies.
- Kotlin/Flutter APK assembly: passed. Flutter settings analysis: no issues.
- Six targeted host regression tests: six passed, zero failed. They cover
  default-on admission without Activity/Wi-Fi/LAN data, explicit disable and UDP
  disable, option-change cancellation/socket release, bounded on-link candidates,
  on-demand registration, existing bypass policies and KCP late-probe handling.
- APK signature verifies with v1/v2; only arm64 native code is packaged. The
  embedded Rust-library hash equals the final compiled library's hash.
- Native export `Java_ffi_FFI_initNativeP2p` and the APK's DEX method signatures
  for `initNativeP2p(Context)` / `nativeP2pInterfaces(Context): String` agree.
- Scope check: MainActivity, MainService, AndroidManifest, SDK/build definitions,
  screen-sharing permissions, desktop settings UI and server code have no diff.
  MainApplication adds only a JNI-reference initialization call; optional LAN
  discovery runs at connection time. Existing-path hooks are listed in ANDROID.md.

Full logs, dependency tree, JNI/DEX checks, source-file hashes and package metadata
are retained alongside the APK. Baseline compiler/Gradle warnings remain in logs.

## Remaining verification

No ADB device was connected. Android P2P in either direction, actual picture/input,
background receiving, Wi-Fi/cellular/VPN transitions, original permission flows,
battery impact and prolonged-session/resource cleanup remain real-device tasks.
Host tests and APK compilation do not establish these results. Follow
[ANDROID.md](ANDROID.md) for normal use, the separate native switch, TUN testing,
rollback and the PC command-line option.
