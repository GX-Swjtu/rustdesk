# Android experimental native P2P validation, 2026-10-14

This is the opt-in Android implementation following desktop client `bea054765`
and client-common `ce2570ba`. It is separate from the earlier Android compatibility
APK, which did not link the native punching module. These Android changes are
currently working-tree changes; no new commit, push or server deployment was made.

## Package and build evidence

Artifacts and full logs are in `artifacts/android-native-20261014/` relative to
the multi-repository workspace. The package is debug-signed, arm64-only, version
1.5.1 (version code 2068), minimum SDK 22 and target SDK 36. The new feature
requires API 24+, is off by default and needs the foreground Wi-Fi/IPv4 conditions
in [ANDROID.md](ANDROID.md). Earlier Android versions retain the original flow.

| Artifact | SHA-256 |
|---|---|
| `rustdesk-native-p2p-android-arm64-beta.apk` | `741998007d014bc69a13b912032827d79d5c8c8b88be6a3ee87d7d941dcf6f93` |
| Final Rust library and APK-embedded `lib/arm64-v8a/librustdesk.so` | `a563ab070944b186de2666c568e15838dcea0eb820b0534936075d1f5eb7f259` |

| Check | Result and evidence |
|---|---|
| Android arm64 Rust release build | Passed; `rust-release.log`, locked/offline dependencies, existing Flutter/hwcodec features. |
| Kotlin/Flutter APK assembly | Passed; `apk-build.log`, `:app:assembleDebug`, no SDK/dependency upgrades. |
| Flutter settings static analysis | Passed with no issues; `flutter-analysis.log`. |
| APK signature | `apksigner verify` passed, v1 and v2; `apk-signature.txt`. Existing META-INF/JAR warnings remain in the log. |
| ABI, SDK and permissions | `apk-badging.txt`: arm64-v8a only; original INTERNET/ACCESS_NETWORK_STATE permissions. No manifest changes. |
| Packaged native code | SHA-256 equals the final built library; exports `Java_ffi_FFI_onNativeP2pNetwork`. |
| Android dependency tree | Includes the existing `rustdesk-native-p2p`; no Windows interface-enumeration dependency introduced. `dependency-tree.txt`. |
| Client regression tests on Windows host | 6 passed, 0 failed; `regression-tests.log`. |
| Android real-device execution | Pending. `adb devices -l` returned no devices. |

The six host tests cover the original feature/proxy/WebSocket bypass, KCP with
late probe responses, explicit Android opt-in and usable-address requirements,
lifecycle/network cancellation with socket release, unchanged-network/settings
transitions, and on-demand signed registration/withdrawal with existing sharing
registration reuse. They exercise the same Rust modules used in Android, but
do not exercise Android system callbacks or establish phone interoperability.

The final additional Android-only user-cancel check was included in the Android
release build. It does not change the Windows-compiled test paths. Compiler and
Gradle warnings are retained in their logs; no unrelated warning cleanup was done.

## Pending real-device acceptance

- Both ID connection directions against the native-capable Windows test clients,
  TUN enabled with the RustDesk app excluded, authenticated `UDP-native`, real
  picture/input/clipboard and relay-traffic evidence.
- Feature off, unsupported peer/server, explicit proxy, forced relay, UDP blocked
  and STUN timeout; confirm fallback and no new multi-socket attempt when disabled.
- Android callback behavior during pause, screen lock, Wi-Fi/default-network/VPN
  changes, plus user cancellation and repeated-attempt FD/memory cleanup.
- Battery/thermal impact of Mode 2/4, sharing permission flows and sustained
  remote control/file transfer. Existing sessions retain upstream lifecycle rules.

The strategy algorithms, protobuf and hbbs/hbbr are unchanged. The precise
existing-path change list and rollback instructions are in [ANDROID.md](ANDROID.md).
The common Android target gate is mirrored in the client's common checkout only;
the server's independent common baseline remains unchanged. Do not interpret this
build record as Android P2P, background-receiving or mobile-network acceptance.
