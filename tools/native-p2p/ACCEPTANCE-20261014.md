# Native UDP punching acceptance, 2026-10-14

This record covers the on-demand extension from the committed v4 client
`6525256b3` / server `428a7e8`. The single signed capability is
`native-udp-punch`. Both endpoints and hbbs must support it. There is one
`enable-native-p2p` switch; incompatible, disabled and Android peers use the
original RustDesk flow. The protobuf envelope name remains `NativeP2pV1`.

## Source and packages

The common patch is `ce2570b` on the client baseline and `a7fd568` on the server
baseline. Their `native-p2p` trees are identical
(`785eeb45d5c67b97f5e6d6e4affdf984e163ea01`). No common-library baseline was merged.
The frp reference remains v0.71.0 (`4a23aa181c1d7e28eecaa8216024ed753b9d27c8`).
Client runtime integration is `2f7d87473`; server integration is `36a0955`.

Packages and raw evidence are in the workspace's
`artifacts/on-demand-20261014/`. SHA-256:

| Artifact | SHA-256 |
| --- | --- |
| `rustdesk-native-udp-punch-windows-x64.zip` | `5213af2a098e84a88b274e73841c438ca1ff90b23fdfe814a41086a956314019` |
| Packaged/installed `librustdesk.dll`, both Windows computers | `3d0e5491070f1a6f4cc1f935900ff0a9779c17932970b450227cc387f69eca74` |
| Linux test `hbbs` | `cdfdb74855abb9b180d3a566ad88357010d493b37ade99ab730fffc3beadb58d` |
| `rustdesk-native-compat-android-arm64-debug.apk` | `36f1790903e1a8210bac90df02b72d2239ca6b513bd69c6dd81c465b6d758587` |
| Android compiled and APK-embedded Rust library | `7d896007ac53fe823a9688971130e643bd099dc7e46848da43bd3b68827dca8d` |

The Windows package reuses the unchanged v4 Flutter release assets; only
`librustdesk.dll` and the feature notice differ. The final client test-only
policy assertions were added after the release DLL build. They change no
release code. The Android native module is compiled out; subsequent changes
inside that excluded module do not change its built runtime code.

Both Windows services use these test binaries and the independent 22116 server.
Production 21115–21119 services and frp were preserved. Both Mihomo TUN adapters
remain enabled. Narrow process/STUN DIRECT configuration is backed up; actual
mapped addresses and wire traffic, rather than rule names, establish the path.

## Module, protocol and platform checks

- Final Windows and Linux common-module suites: 23/23 each.
- Client focused tests: 2/2, covering disabled/proxy/WebSocket early fallback
  and late authenticated UDP probes before the upstream KCP handshake.
- Server admission integration: correct key accepted; absent/wrong server
  keys, unsigned/forged/replayed withdrawals rejected; signed withdrawal
  cancels the attempt; original admission-key checks remain effective.
- Common tests cover exact/absent/old capability negotiation, the one-second
  old-server bound, signatures, participant eligibility, TCP source evidence,
  candidate limits, replay, quotas, cancellation and timeouts.
- Windows x64 release DLL and Linux hbbs/diagnostics built with locked
  dependencies. No public dependency upgrade was included.
- Android arm64 Rust release and debug APK built. The APK contains only
  arm64-v8a, its embedded Rust hash matches the compiled library, and APK
  signature verification passes. The target dependency graph excludes
  `rustdesk-native-p2p`. Android true-device testing remains deferred at the
  user's request; compilation and protocol exclusions are not a device test.

Build commands and toolchain requirements are in [README.md](README.md).
Detailed logs include `windows-module-r4.log`, `linux-module-r4.log`,
`windows-client-final-tests.log`, `server-admission.json`, `android-rust.log`,
`android-apk.log`, `android-verification.json` and `android-dependency-tree.txt`.

## Isolated Linux network tests

`nat-lab.py` uses isolated namespaces, real conntrack/SNAT/nftables and netem.
The diagnostic shares the production punching module and upstream
`src/kcp_stream.rs`; success requires signed identity, mutual nonce proof and
64 KiB KCP communication. It does not reward production strategy history.
Binaries were frozen before the final run; results record their hashes.

| Final scenario | Successful authenticated KCP sessions |
| --- | ---: |
| Mode 0 / Direct | 10/10 |
| Mode 1 / Predict | 10/10 |
| Mode 2 / Birthday | 10/10 |
| Mode 3 / BothPredict | 10/10 |
| Mode 4 / Mixed | 10/10 |
| Multiple verified exits | 9/10 |
| 1% loss, 12 ms ± 4 ms delay | 10/10 |
| LAN candidates | 10/10 |
| Public UDP receiver | 10/10 |

The multi-exit trial 6 timed out; it is retained as a failure, not removed from
the denominator. All final success scenarios meet the requested 9/10 threshold.
Six rejection/cleanup scenarios also pass: blocked UDP, unverified exit,
unpredictable dual HardNAT, cancellation, bad proof and blackhole timeout.
The final one-per-scenario rerun reports no remaining sockets or namespaces;
the earlier five negative scenarios also have ten runs each.

LAN/public fixtures inject an explicit veth inventory only in the diagnostic.
Shipped clients enumerate physical adapters. These results do not establish
physical-LAN behavior on every adapter model or real dual-HardNAT/multi-exit
network coverage.

All earlier groups remain in `lab-*` and `lab-summary.json`. Initial multi-exit
and loss groups missed the threshold. Fixes kept target budgets shared across
exits, weighted duplicate observed exit IPs, retried transient coordination
errors within the original deadline, and moved lab output from a failing host
filesystem to ext4. An earlier max group was 3/6: a non-monotonic, small port pool
was misclassified as predictable. Prediction now requires three strictly
monotonic observations on the same public IP. The final Windows/max diagnostic
group (`cross-final`) is 6/6, three connections in each direction. These
diagnostics are separate from the Windows desktop results below.

## Actual Windows desktop connections

Times below are China Standard Time (UTC+08:00), 2026-10-14. Connections use
RustDesk IDs, the existing login/permission path and retained Mihomo TUN.
Receivers and the server relay capture were started before callers.

| Direction / interval | Native secure KCP | Automated picture check | Additional evidence |
| --- | ---: | ---: | --- |
| Home → school, 03:06:19–03:11:29 | 10/10 | 9/10 | Trial 7 desktop visually confirmed and saved |
| School → home, 03:11:56–03:16:14 | 10/10 | 10/10 | Independent cold callers |

Home trial 7's raw harness outcome remains `timeout`: its static screen did
not emit the expected `Set fps` log. Authentication and desktop login succeeded;
`home-final-trial-07-desktop.png` records the rendered desktop. The conservative
automated totals alone meet the requested threshold. Median secure-KCP time
from launch is 6.82 s / 12.68 s; maximum is 13.87 s / 19.87 s respectively.
All twenty caller snapshots report zero TCP connections to test hbbr.

`windows-cold-summary.json` retains every trial, timing, strategy, peer UDP
address and the distinction between automated and visual evidence. Original
logs remain under `cold-home-final` and `cold-school-final`.

Forced relay and a separately disabled peer each completed an original-path
desktop connection, with one relay connection and no native UDP allocation.
The disabled-peer recheck took about nine seconds including desktop startup,
without the new 45-second window. Both user and LocalService options were
checked after disabling and after restoration.

Test hygiene matters: upstream `--relay` persists `force-always-relay=Y` in the
peer configuration. The first disabled-peer test and a preliminary soak were
therefore excluded. Another preliminary soak used the original path before
capability advertisements converged and was also excluded. After backing up that configuration and
clearing only this flag, disabling was tested independently. The final soak
started only after fresh signed enabled advertisements from both services,
and its harness required native secure KCP and desktop login before starting
the duration clock. No strategy delay was inferred from a stale advertisement.

## Continuous session, input and transfer

The accepted continuous session began at 03:25:48.258 after native secure KCP
and login. Its home UDP port is 49479, public mapping `home-public:51777`, and
school peer `school-public:2268`. Mode 2 confirmed after 224 probes. Exact
addresses remain in the local raw evidence.

Monitoring ended at 03:58:10.139: 1,941.88 seconds (32 min 22 s). All 60 samples
show the caller alive, TUN up, no relay TCP connection, and the same desktop UDP
port retained. The log contains no disconnect or fallback. The two native KCP
sessions in that log are the desktop and the separate file transfer. A second
mouse click after 30 minutes updated the remote counter to two; see
`soak-after-30-minutes.png` and `soak-complete.png`. `soak-summary.json` retains
the timing, input report and transfer verification.

The remote local-only test page recorded a mouse click, six keyboard events
and two paste events. The exact typed/pasted markers and counters are visible
in `keyboard-clipboard-mouse.png`; this evidence uses the actual remote desktop
path. The matching page event report was saved through Edge's normal download
prompt and read back as `keyboard-clipboard-mouse-download.json`.

A new 104,857,600-byte sample was sent through the RustDesk file-transfer UI at
03:36:49, into an initially empty school test directory. School-side read-only
hash verification at 03:37:23 confirmed SHA-256
`e8e2ff79e13bdbf6ef5253caa3ac2d3d32d79b70d54e17ee84d76267e76ad393`, matching
the local source. Only local staging used a filesystem copy; the payload was
not uploaded with SSH/SCP. The file-transfer connection independently completed
native secure KCP at 03:28:18.858 to `school-public:2082`, public home mapping
`home-public:51799`. `transfer-complete.png`, source/receiver JSON and client logs
record completion. The desktop session continued while the transfer ran.

### Network evidence

The server's relay capture ran from 02:55:30 until its scoped stop at 03:59.
It contains zero TCP-22117 packets in all twenty final native cold intervals,
the accepted soak, and the file transfer. Positive controls contain 1,486
packets / 1,372,970 TCP payload bytes for forced relay and 1,359 packets /
1,315,288 bytes for the independent disabled-peer recheck. The capture filter
therefore demonstrably observed the relay path when it was used.

Both endpoint captures include physical Wi-Fi packets, separately from TUN
packets, to the other endpoint's public IP/port. The fixed desktop tuple has
traffic in every minute bucket 0–31 at both physical endpoints. School's
physical file-transfer receive tuple records over 116 MB of UDP payload
observations from the public home mapping, alongside the exact 100 MiB file
hash match. NIC/802.11 retransmissions may duplicate observations; byte totals
are path evidence, not an application-throughput measurement. These native
tuples terminate at the two devices, not hbbr, frps or TURN. Existing unrelated
frp traffic is not counted as RustDesk data.

`capture-summary.json` decodes both raw-IP TUN frames and Wi-Fi LLC/SNAP frames
emitted by pktmon, and retains the physical source addresses and per-minute
counts. Endpoint capture began during home cold trial 3; the server capture
covers all final cold trials. Endpoint clocks are not precisely synchronised.

Both endpoint captures stopped and their temporary filters were removed;
server tcpdump also exited. After the first home stop invocation returned a
cancelled UAC elevation, the user requested another invocation and approved it.
Home cleanup completed at 08:10:38. `home-udp.pcapng` covers 03:07:36–08:10:29,
including all 32 accepted-soak minute buckets; `home-checkpoint.pcapng` is also
preserved. The stop script removed the task's named filter and confirmed pktmon
idle with no filters. RustDesk remained running and Mihomo TUN remained up.

## Rollback and remaining coverage

Use `enable-native-p2p=N` through the existing service-synchronised setting to
return to original RustDesk behavior. There is no separate V2 switch. Restoring
v4 native behavior requires restoring both prior client packages and prior test
hbbs, since the single capability deliberately excludes older experimental
packages. Original installation rollback remains unchanged.

`Update-TestClient.ps1 -Package <saved-previous-client.zip>` restores a saved
test package. The original v4 checkpoints are `update-20261014-025429` at home
and `update-20261014-025411` at school; later backups contain intermediate test
builds. Preserve private configuration backups outside Git. Test hbbs's
`hbbs.before-native-udp-punch` and its scoped `--restore` helper preserve the
production server and the test database/key. The artifact delivery notes give
the exact paths used here.

Real Android interoperability and real dual-HardNAT/multiple-public-exit
acceptance remain pending. UDP-only exits that cannot be observed by signed
TCP are intentionally unsupported and fall back. Traversal remains probabilistic;
there is no background retry, relay migration or promise of universal NAT support.

## Final minimization review

Compared with v4, changed existing production paths are limited to:

| File / path | Necessity |
| --- | --- |
| Client `src/native_p2p.rs` | Emit the one signed capability and enable flag. |
| Client `src/client/native_p2p.rs` | Report the unversioned `UDP-native` transport; expand the existing focused policy test. |
| Both `libs/hbb_common` pins | Select identical native patches on the two original baselines. |
| Both `Cargo.lock` files | Wire existing libc/windows-sys versions for physical adapter enumeration; client adds the already selected windows-sys 0.59 package. |
| Common `native-p2p/` only | Five modes, bounded shared budgets, candidates, TCP source proofs, scoring and tests; remove the old experimental capability branch. |
| Existing diagnostic/rule/test/docs files | Exercise the agreed network cases and include the diagnostic in narrow TUN rules. |

The diagnostic and namespace lab are additions requested by the acceptance
plan. No existing media, UI, KCP, TCP/WebRTC, Android permission/JNI or hbbr
forwarding path was refactored. This expansion adds no hbbs wrapper hook,
protobuf schema field or port. The common patch changes 16 feature-only files
with 994 additions and 1091 deletions, including documentation and tests.
