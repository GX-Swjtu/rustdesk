import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:visibility_detector/visibility_detector.dart';

/// Shared by desktop, Android and the per-mapping forwarding panel. It only
/// observes network work; visibility never starts or stops the Rust controller.
class NativeP2pWaiting extends StatefulWidget {
  const NativeP2pWaiting(
      {super.key,
      required this.readSnapshot,
      required this.control,
      required this.translate,
      this.onCancel,
      this.embedded = false});
  final Future<String> Function() readSnapshot;
  final Future<bool> Function(String id, String action) control;
  final String Function(String) translate;
  final VoidCallback? onCancel;
  final bool embedded;

  @override
  State<NativeP2pWaiting> createState() => _NativeP2pWaitingState();
}

class _NativeP2pWaitingState extends State<NativeP2pWaiting>
    with WidgetsBindingObserver {
  Timer? _timer;
  bool _visible = true, _foreground = true, _inFlight = false;
  List<Map<String, dynamic>> _snapshots = [];
  final Map<String, String> _submitted = {};
  final Set<String> _expanded = {};
  String? _readError;

  String tr(String key) => widget.translate(key);

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _foreground = WidgetsBinding.instance.lifecycleState == null ||
        WidgetsBinding.instance.lifecycleState == AppLifecycleState.resumed;
    _refreshTimer();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _foreground = state == AppLifecycleState.resumed;
    _refreshTimer();
  }

  void _refreshTimer() {
    _timer?.cancel();
    if (!_visible || !_foreground) return;
    unawaited(_read());
    _timer = Timer.periodic(const Duration(milliseconds: 500), (_) => _read());
  }

  Future<void> _read() async {
    if (_inFlight || !mounted || !_visible || !_foreground) return;
    _inFlight = true;
    try {
      final data = jsonDecode(await widget.readSnapshot()) as List;
      if (!mounted || !_visible || !_foreground) return;
      final snapshots = data
          .whereType<Map>()
          .map((e) => Map<String, dynamic>.from(e))
          .toList();
      setState(() {
        _snapshots = snapshots;
        _readError = null;
      });
    } catch (_) {
      if (mounted)
        setState(
            () => _readError = tr('P2P diagnostics temporarily unavailable'));
    } finally {
      _inFlight = false;
    }
  }

  Future<void> _request(Map<String, dynamic> flow, String action) async {
    final id = flow['id'] as String;
    if (_submitted.containsKey(id)) return;
    setState(() => _submitted[id] = action);
    // A shared terminal view must close through the existing reference count.
    // Only forwarding sub-connections are cancelled directly by flow id.
    if (action == 'cancel' && !widget.embedded) {
      widget.onCancel?.call();
      return;
    }
    try {
      final accepted = await widget.control(id, action);
      if (!mounted) return;
      if (accepted && action == 'cancel' && !widget.embedded)
        widget.onCancel?.call();
      if (!accepted) setState(() => _submitted.remove(id));
      unawaited(_read());
    } catch (_) {
      if (mounted)
        setState(() {
          _submitted.remove(id);
          _readError = tr('P2P diagnostics temporarily unavailable');
        });
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  String seconds(dynamic ms) =>
      ((ms is num ? ms : 0) / 1000).toStringAsFixed(1);

  String stageText(String? stage) => tr(const {
        'slot': 'Waiting for another P2P attempt to finish',
        'prepare': 'Preparing P2P session',
        'network': 'Discovering UDP mappings',
        'peer': 'Waiting for peer candidates',
        'probe': 'Probing P2P strategy',
        'kcp': 'Establishing KCP',
        'secure': 'Verifying device identity',
        'authenticated': 'P2P identity verified',
        'backoff': 'Waiting to retry P2P',
        'waiting': 'Waiting for connection choice',
        'relay': 'Stopping P2P and connecting through relay',
        'cancelling': 'Stopping P2P',
      }[stage] ??
      'Unknown');

  Widget _network(String title, dynamic value) {
    final network = value is Map ? value : const {};
    final candidates =
        network['candidates'] is Map ? network['candidates'] as Map : const {};
    final profile =
        network['profile'] is Map ? network['profile'] as Map : const {};
    final observed = (candidates['public'] as List?) ?? [];
    final local = (candidates['local'] as List?) ?? [];
    final samplesPerIp = <String, int>{};
    for (final address in observed) {
      final ip = address.toString().split(':').first;
      samplesPerIp[ip] = (samplesPerIp[ip] ?? 0) + 1;
    }
    final predictionObserved = samplesPerIp.values.any((count) => count >= 3);
    String known(dynamic value) =>
        value == null ? tr('Unknown') : tr(value == true ? 'Yes' : 'No');
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(tr(title), style: const TextStyle(fontWeight: FontWeight.bold)),
      SelectableText(
          '${tr('LAN candidates')}: ${local.isEmpty ? tr('Unknown') : local.join(', ')}'),
      SelectableText(
          '${tr('UDP mappings in observation order')}: ${observed.isEmpty ? tr('Unknown') : observed.asMap().entries.map((e) => '${e.key + 1}. ${e.value}').join(' → ')}'),
      Text(
          '${tr('Stable mapping')}: ${known(observed.length >= 2 ? profile['easy'] : null)} · ${tr('Predictable port')}: ${known(predictionObserved ? profile['predictable'] : null)}'),
      Text(
          '${tr('Public exits')}: ${(profile['ips'] as List?)?.join(', ') ?? tr('Unknown')}'),
      if (observed.length < 3 || profile['predictable'] != true)
        Text(tr('Insufficient evidence for port prediction'),
            style: Theme.of(context).textTheme.bodySmall),
      const SizedBox(height: 12),
    ]);
  }

  Widget _details(Map<String, dynamic> flow) {
    final strategy =
        flow['strategy'] is Map ? flow['strategy'] as Map : const {};
    final behavior =
        strategy['behavior'] is Map ? strategy['behavior'] as Map : const {};
    final probes = flow['probes'] is Map ? flow['probes'] as Map : const {};
    const modes = {
      'Direct': 'Mode 0 · Direct',
      'Predict': 'Mode 1 · Port prediction',
      'Birthday': 'Mode 2 · Random ports',
      'BothPredict': 'Mode 3 · Both predict',
      'Mixed': 'Mode 4 · Mixed'
    };
    final sender = strategy['sender'];
    final role = sender == null
        ? tr('Unknown')
        : tr(sender == true ? 'Sender' : 'Receiver');
    final peerRole = sender == null
        ? tr('Unknown')
        : tr(sender == true ? 'Receiver' : 'Sender');
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      _network('Local network', flow['local']),
      _network('Peer network', flow['peer']),
      SelectableText(
          '${tr('Strategy')}: ${tr(modes[behavior['mode']] ?? 'Unknown')}\n'
          '${tr('Local role')}: $role · ${tr('Peer role')}: $peerRole\n'
          'TTL (${tr('Receiver')}): ${behavior['ttl'] ?? tr('Unknown')} · '
          '${tr('Send delay')} (${tr('Sender')}): ${behavior['delay_ms'] ?? tr('Unknown')} ms\n'
          '${tr('Probes sent')}: ${probes['sent'] ?? 0}/8192 · '
          '${tr('Authenticated UDP replies')}: ${probes['authenticated'] ?? 0}\n'
          '${tr('Probe sockets')}: ${probes['sockets'] ?? 0}/257'),
      const SizedBox(height: 8),
      Text(
          tr('UDP confirmation, KCP and identity verification are separate stages'),
          style: Theme.of(context).textTheme.bodySmall),
      ExpansionTile(title: Text(tr('Recent P2P rounds')), children: [
        SelectableText(
            const JsonEncoder.withIndent('  ').convert(flow['rounds'] ?? [])),
      ]),
      ExpansionTile(title: Text(tr('P2P stage records')), children: [
        SelectableText(
            const JsonEncoder.withIndent('  ').convert(flow['events'] ?? [])),
      ]),
      TextButton.icon(
          icon: const Icon(Icons.copy, size: 18),
          label: Text(tr('Copy P2P diagnostics')),
          onPressed: () => Clipboard.setData(ClipboardData(
              text: const JsonEncoder.withIndent('  ').convert(flow)))),
    ]);
  }

  Widget _flow(Map<String, dynamic> flow) {
    final id = flow['id'] as String;
    final action = _submitted[id];
    final stage = flow['stage'] as String?;
    final parked = stage == 'waiting';
    final busy = action != null ||
        _submitted.containsValue('relay') ||
        stage == 'relay' ||
        stage == 'cancelling';
    final expanded = _expanded.contains(id);
    return Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
                if (!parked)
                  const Padding(
                      padding: EdgeInsets.only(right: 12, top: 2),
                      child: SizedBox(
                          width: 20,
                          height: 20,
                          child: CircularProgressIndicator(strokeWidth: 2))),
                Expanded(
                    child: Text(
                        tr(parked
                            ? 'Waiting for connection choice'
                            : 'Trying P2P first'),
                        style: Theme.of(context).textTheme.titleMedium)),
              ]),
              if (flow['mapping'] != null)
                Text('${tr('Local Port')}: ${flow['mapping']}'),
              const SizedBox(height: 12),
              if (!parked || busy)
                Text(action == 'relay'
                    ? tr('Stopping P2P and connecting through relay')
                    : action == 'cancel'
                        ? tr('Stopping P2P')
                        : stageText(stage)),
              Text(
                  '${tr('P2P round')} ${flow['round']} · ${tr('Total wait')}: ${seconds(flow['elapsed_ms'])} s'),
              if (!parked && !busy && stage != 'backoff' && stage != 'slot')
                Text(
                    '${tr('This round')}: ${seconds(flow['round_ms'])} / ${seconds(flow['limit_ms'])} s'),
              if (flow['retry_ms'] != null && !busy)
                Text(
                    '${tr('Next P2P attempt in')}: ${seconds(flow['retry_ms'])} s'),
              if (flow['reason'] != null)
                Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: SelectableText(
                        '${tr('Last P2P result')}: ${tr(flow['reason'].toString())}')),
              TextButton(
                  onPressed: () => setState(() {
                        expanded ? _expanded.remove(id) : _expanded.add(id);
                      }),
                  child: Text(
                      tr(expanded ? 'Hide P2P details' : 'Show P2P details'))),
              if (expanded) _details(flow),
              Wrap(spacing: 12, runSpacing: 8, children: [
                TextButton(
                    onPressed: busy ? null : () => _request(flow, 'cancel'),
                    child: Text(tr('Cancel'))),
                OutlinedButton(
                    onPressed: busy ? null : () => _request(flow, 'relay'),
                    child: Text(tr('Relay connection'))),
              ]),
            ]));
  }

  @override
  Widget build(BuildContext context) {
    final active = _snapshots.where((flow) => flow['active'] == true).toList();
    return VisibilityDetector(
        key: ValueKey(this),
        onVisibilityChanged: (info) {
          final visible = info.visibleFraction > 0;
          if (_visible != visible) {
            _visible = visible;
            _refreshTimer();
          }
        },
        child: ConstrainedBox(
            constraints: BoxConstraints(
                maxWidth: 540,
                maxHeight: widget.embedded
                    ? 320
                    : MediaQuery.sizeOf(context).height * .75),
            child: SingleChildScrollView(
                child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                  if (active.isEmpty && !widget.embedded) ...[
                    const Center(child: CircularProgressIndicator()),
                    const SizedBox(height: 20),
                    Center(child: Text(tr('Connecting...'))),
                    TextButton(
                        onPressed: widget.onCancel, child: Text(tr('Cancel'))),
                  ],
                  ...active.map(_flow),
                  if (_readError != null) Text(_readError!),
                ]))));
  }
}
