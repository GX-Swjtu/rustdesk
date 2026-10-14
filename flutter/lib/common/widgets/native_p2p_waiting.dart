import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:visibility_detector/visibility_detector.dart';

/// Observes the existing controller. Hiding this panel never stops network work.
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
  final Map<String, int> _tabs = {};
  final Map<String, ScrollController> _scroll = {};
  String? _readError, _copied;

  String tr(String key) => widget.translate(key);
  Map data(dynamic value) => value is Map ? value : const {};
  List items(dynamic value) => value is List ? value : const [];
  String number(dynamic value) => value is num ? '$value' : tr('Unknown');
  Color get accent => Theme.of(context).colorScheme.primary;
  Color get muted => Theme.of(context).colorScheme.onSurface.withOpacity(.68);
  Color get line => Theme.of(context).dividerColor.withOpacity(.18);
  TextStyle get small => TextStyle(fontSize: 12, color: muted, height: 1.4);

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
      final decoded = jsonDecode(await widget.readSnapshot()) as List;
      if (!mounted || !_visible || !_foreground) return;
      final snapshots = decoded
          .whereType<Map>()
          .map((e) => Map<String, dynamic>.from(e))
          .where((e) => e['id'] is String)
          .toList();
      final ids = snapshots.map((e) => e['id'] as String).toSet();
      setState(() {
        _snapshots = snapshots;
        _readError = null;
        _expanded.removeWhere((id) => !ids.contains(id));
        _tabs.removeWhere((id, _) => !ids.contains(id));
        _submitted.removeWhere((id, _) => !ids.contains(id));
        // Retained snapshots are bounded by the controller, as is this UI state.
        for (final id
            in _scroll.keys.where((id) => !ids.contains(id)).toList()) {
          final controller = _scroll.remove(id)!;
          // Let the removed flow detach from its scroll view first.
          WidgetsBinding.instance
              .addPostFrameCallback((_) => controller.dispose());
        }
      });
    } catch (_) {
      if (mounted && _visible && _foreground) {
        setState(
            () => _readError = tr('P2P diagnostics temporarily unavailable'));
      }
    } finally {
      _inFlight = false;
    }
  }

  Future<void> _request(Map flow, String action) async {
    final id = flow['id'] as String;
    final previous = _submitted[id];
    if (previous == 'cancel' ||
        (action == 'relay' &&
            (previous != null || _submitted.containsValue('relay')))) return;
    setState(() => _submitted[id] = action);
    // Shared terminal views retain the original close/reference-count path.
    if (action == 'cancel' && !widget.embedded) {
      widget.onCancel?.call();
      return;
    }
    try {
      final accepted = await widget.control(id, action);
      if (!mounted || _submitted[id] != action) return;
      if (!accepted) setState(() => _submitted.remove(id));
      unawaited(_read());
    } catch (_) {
      if (!mounted || _submitted[id] != action) return;
      setState(() {
        _submitted.remove(id);
        _readError = tr('P2P diagnostics temporarily unavailable');
      });
    }
  }

  Future<void> _copy(Map flow) async {
    try {
      await Clipboard.setData(ClipboardData(
          text: const JsonEncoder.withIndent('  ').convert(flow)));
      if (mounted) setState(() => _copied = flow['id'] as String);
    } catch (_) {
      if (mounted) {
        setState(() => _readError = tr('Unable to copy diagnostics'));
      }
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    for (final controller in _scroll.values) {
      controller.dispose();
    }
    super.dispose();
  }

  String duration(dynamic ms, {bool clock = false}) {
    if (ms is! num) return tr('Unknown');
    final seconds = (ms / 1000).ceil().clamp(0, 1 << 40);
    if (!clock) return '$seconds s';
    return '${(seconds ~/ 60).toString().padLeft(2, '0')}:${(seconds % 60).toString().padLeft(2, '0')}';
  }

  String stage(Map flow) => _submitted[flow['id']] == 'cancel'
      ? 'cancelling'
      : _submitted[flow['id']] == 'relay'
          ? 'relay'
          : flow['stage'] as String? ?? '';
  String stageText(String stage) => tr(const {
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
      'Connecting...');
  String reason(dynamic value) {
    final text = value?.toString().toLowerCase() ?? '';
    if (text.contains('coordinator closed')) {
      return tr('The coordination server closed the connection');
    }
    if (text.contains('timed out') ||
        text.contains('timeout') ||
        text.contains('deadline has elapsed')) {
      return tr('The connection attempt timed out');
    }
    if ((text.contains('unknown') && text.contains('capability')) ||
        text.contains('capability not registered')) {
      return tr('Waiting for the peer to register its P2P capability');
    }
    if (text.contains('registration expired')) {
      return tr('Waiting for the peer to refresh its registration');
    }
    if (text.contains('device busy') || text.contains('session limit')) {
      return tr('The peer or server is busy');
    }
    if (text.contains('no supported native udp') ||
        text.contains('no applicable strategy') ||
        text.contains('no eligible wan')) {
      return tr('No strategy is available for this network combination');
    }
    if (text.contains('disabled on the peer')) {
      return tr('The peer has turned off native P2P');
    }
    if (text.contains('explicitly declined') ||
        text.contains('capability not negotiated')) {
      return tr(
          'The server or peer explicitly declined the native P2P capability');
    }
    if (text.contains('p2p identity verified')) {
      return tr('P2P identity verified');
    }
    if (text.contains('invalid signature') ||
        text.contains('identity mismatch') ||
        text.contains('identity handshake failed') ||
        text.contains('key mismatch')) {
      return tr('The device or server identity could not be verified');
    }
    if (text.contains('connection refused') ||
        text.contains('network unreachable')) {
      return tr('The network connection could not be established');
    }
    return tr('The P2P attempt did not complete');
  }

  String mode(Map strategy) => tr(const {
        'Direct': 'Mode 0 · Direct',
        'Predict': 'Mode 1 · Port prediction',
        'Birthday': 'Mode 2 · Random ports',
        'BothPredict': 'Mode 3 · Both predict',
        'Mixed': 'Mode 4 · Mixed',
      }[data(strategy['behavior'])['mode']] ??
      'Unknown');
  String mapping(Map network) {
    final observed = items(data(network['candidates'])['public']),
        profile = data(network['profile']);
    if (profile['easy'] == null || observed.length < 2) return tr('Unknown');
    if (items(profile['ips']).length > 1) return tr('Multiple public exits');
    return tr(profile['easy'] == true ? 'Stable mapping' : 'Variable mapping');
  }

  String prediction(Map network) {
    final profile = data(network['profile']),
        observed = items(data(network['candidates'])['public']);
    if (profile.isEmpty || observed.isEmpty) return tr('Unknown');
    if (profile['easy'] == true && observed.length >= 2) {
      return tr('Port prediction is not needed');
    }
    final counts = <String, int>{};
    for (final address in observed) {
      final ip = address.toString().split(':').first;
      counts[ip] = (counts[ip] ?? 0) + 1;
    }
    if (!counts.values.any((n) => n >= 3)) {
      return tr('Insufficient evidence for port prediction');
    }
    if (profile['predictable'] is! bool) return tr('Unknown');
    return tr(profile['predictable'] == true
        ? 'Predictable port'
        : 'No predictable port pattern observed');
  }

  Widget card(Widget child, {Color? tint}) => Container(
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
          color: tint ?? Theme.of(context).cardColor,
          borderRadius: BorderRadius.circular(10),
          border: Border.all(color: line)),
      child: child);
  Widget pair(Widget local, Widget peer) =>
      LayoutBuilder(builder: (_, constraints) {
        if (constraints.maxWidth < 560 ||
            MediaQuery.textScalerOf(context).scale(14) > 21) {
          return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [local, const SizedBox(height: 10), peer]);
        }
        return Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Expanded(child: local),
          const SizedBox(width: 12),
          Expanded(child: peer)
        ]);
      });
  Widget label(String text) => Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child:
          Text(tr(text), style: const TextStyle(fontWeight: FontWeight.w600)));
  Widget entry(String title, String value, {bool address = false}) => Padding(
      padding: const EdgeInsets.symmetric(vertical: 5),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(tr(title), style: small),
        const SizedBox(height: 2),
        SelectionArea(
            child: Text(value,
                style: TextStyle(
                    fontFamily: address ? 'monospace' : null,
                    fontSize: 14,
                    height: 1.45))),
      ]));
  Widget networkSummary(String title, dynamic value, IconData icon) {
    final network = data(value), ips = items(data(network['profile'])['ips']);
    final behavior = mapping(network), predictionText = prediction(network);
    return card(Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Row(children: [
        Icon(icon, size: 18, color: muted),
        const SizedBox(width: 8),
        Expanded(child: Text(tr(title), style: small))
      ]),
      const SizedBox(height: 8),
      Text(
          behavior == tr('Unknown')
              ? '${tr('Mapping behavior')}: $behavior'
              : behavior,
          style: const TextStyle(fontWeight: FontWeight.w600)),
      const SizedBox(height: 4),
      Text(
          predictionText == tr('Unknown')
              ? '${tr('Port prediction')}: $predictionText'
              : predictionText,
          style: small),
      const SizedBox(height: 4),
      Text('${tr('Public exits')}: ${ips.isEmpty ? tr('Unknown') : ips.length}',
          style: small),
    ]));
  }

  Widget networkDetails(String title, dynamic value) {
    final network = data(value), candidates = data(network['candidates']);
    final observed = items(candidates['public']),
        local = items(candidates['local']);
    final ips = items(data(network['profile'])['ips']);
    return card(Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      label(title),
      entry('Mapping behavior', mapping(network)),
      entry('Port prediction', prediction(network)),
      entry('LAN candidates', local.isEmpty ? tr('Unknown') : local.join('\n'),
          address: true),
      entry('Public exits', ips.isEmpty ? tr('Unknown') : ips.join('\n'),
          address: true),
      const SizedBox(height: 8),
      Text(tr('UDP mappings in observation order'), style: small),
      if (observed.isEmpty)
        Padding(
            padding: const EdgeInsets.only(top: 6), child: Text(tr('Unknown'))),
      for (final observation in observed.asMap().entries)
        Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
              SizedBox(
                  width: 26,
                  child: Text('${observation.key + 1}', style: small)),
              Expanded(
                  child: SelectionArea(
                      child: Text('${observation.value}',
                          style: const TextStyle(
                              fontFamily: 'monospace',
                              fontSize: 13,
                              height: 1.5)))),
            ])),
    ]));
  }

  Widget strategyDetails(Map flow) {
    final strategy = data(flow['strategy']),
        behavior = data(strategy['behavior']),
        probes = data(flow['probes']);
    final sender = strategy['sender'];
    String role(bool local) => sender is bool
        ? tr(sender == local ? 'Sender' : 'Receiver')
        : tr('Unknown');
    const descriptions = {
      'Direct': 'Both peers probe the observed addresses directly.',
      'Predict': 'One peer probes ports predicted from repeated observations.',
      'Birthday': 'Random port probes meet the peer\'s additional UDP sockets.',
      'BothPredict': 'Both peers probe predicted port ranges.',
      'Mixed':
          'Port prediction is combined with random probes and additional sockets.',
    };
    final reached = items(flow['events'])
        .whereType<Map>()
        .where((event) => event['round'] == flow['round'])
        .map((event) => event['stage'])
        .toSet()
      ..add(flow['stage']);
    final udp = probes['authenticated'];
    String milestone(bool confirmed, bool running) => tr(confirmed
        ? 'Confirmed'
        : running
            ? 'In progress'
            : 'Not yet confirmed');
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      card(Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(mode(strategy),
            style: const TextStyle(fontWeight: FontWeight.w600)),
        const SizedBox(height: 6),
        Text(
            tr(descriptions[behavior['mode']] ??
                'Strategy information is not available yet'),
            style: small),
        if (strategy['lan_only'] == true)
          Text(tr('Local network candidates only'), style: small),
      ])),
      const SizedBox(height: 12),
      pair(card(entry('Local probe role', role(true))),
          card(entry('Peer probe role', role(false)))),
      const SizedBox(height: 8),
      Text(tr('Probe roles are independent of who controls the desktop'),
          style: small),
      const SizedBox(height: 12),
      card(Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        entry(
            'Receiver TTL',
            behavior['ttl'] == 0
                ? tr('System default')
                : number(behavior['ttl'])),
        entry('Sender delay', duration(behavior['delay_ms'])),
        entry('Probes sent',
            probes['sent'] is num ? '${probes['sent']} / 8192' : tr('Unknown')),
        entry('Authenticated UDP replies', number(probes['authenticated'])),
        entry(
            'Probe sockets used this round',
            probes['sockets'] is num
                ? '${probes['sockets']} / 257'
                : tr('Unknown')),
      ])),
      const SizedBox(height: 10),
      card(Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        entry(
            'UDP authentication',
            udp is! num
                ? tr('Unknown')
                : milestone(udp > 0, stage(flow) == 'probe')),
        entry(
            'KCP channel',
            milestone(
                reached.contains('secure') || reached.contains('authenticated'),
                stage(flow) == 'kcp')),
        entry(
            'Device identity',
            milestone(
                reached.contains('authenticated'), stage(flow) == 'secure')),
      ])),
      const SizedBox(height: 10),
      Text(
          tr('UDP confirmation, KCP and identity verification are separate stages'),
          style: small),
    ]);
  }

  Widget rawReason(String text, String id) => ExpansionTile(
          key: PageStorageKey('reason-$id'),
          tilePadding: EdgeInsets.zero,
          childrenPadding: const EdgeInsets.only(bottom: 10),
          title: Text(tr('Technical message'), style: small),
          children: [
            Align(
                alignment: Alignment.centerLeft,
                child: SelectionArea(
                    child: Text(text,
                        style: const TextStyle(
                            fontSize: 12,
                            fontFamily: 'monospace',
                            height: 1.5))))
          ]);
  Widget history(Map flow) {
    final rounds =
        items(flow['rounds']).whereType<Map>().toList().reversed.take(20);
    final events = items(flow['events']).whereType<Map>().toList();
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      if (flow['reason'] != null)
        card(Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          label('Last P2P result'),
          Text(reason(flow['reason'])),
          rawReason('${flow['reason']}', '${flow['id']}-last')
        ])),
      const SizedBox(height: 12),
      label('Recent P2P rounds'),
      if (rounds.isEmpty) Text(tr('No completed rounds yet'), style: small),
      for (final round in rounds)
        Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: card(
                Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Wrap(spacing: 16, runSpacing: 4, children: [
                Text('${tr('P2P round')} ${number(round['round'])}',
                    style: const TextStyle(fontWeight: FontWeight.w600)),
                Text(duration(round['elapsed_ms']), style: small),
              ]),
              const SizedBox(height: 6),
              Text(mode(data(round['strategy'])), style: small),
              const SizedBox(height: 6),
              Text(reason(round['reason'])),
              if (round['reason'] != null)
                rawReason(
                    '${round['reason']}', '${flow['id']}-${round['round']}'),
            ]))),
      const SizedBox(height: 14),
      label('P2P stage records'),
      if (events.isEmpty) Text(tr('No stage records yet'), style: small),
      for (final event
          in events.skip(events.length > 200 ? events.length - 200 : 0))
        Padding(
            padding: const EdgeInsets.symmetric(vertical: 7),
            child:
                Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(
                  '${duration(event['ms'], clock: true)} · ${tr('P2P round')} ${number(event['round'])}',
                  style: small),
              const SizedBox(height: 2),
              Text(event['stage'] is String
                  ? stageText(event['stage'])
                  : tr(const {
                        'Direct': 'Direct channel ready',
                        'Relay': 'Relay connection',
                        'Cancel': 'Stopping P2P',
                        'Legacy': 'Connecting...',
                      }[event['choice']] ??
                      'Unknown')),
            ])),
    ]);
  }

  Widget phases(Map flow) {
    final index = const {
          'prepare': 0,
          'network': 1,
          'peer': 1,
          'probe': 2,
          'kcp': 2,
          'secure': 3,
          'authenticated': 4
        }[stage(flow)] ??
        -1;
    const titles = [
      'Preparation',
      'Network discovery',
      'Direct connection',
      'Security check'
    ];
    return Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
      for (var i = 0; i < titles.length; i++)
        Expanded(
            child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 3),
                child: Column(children: [
                  Container(
                      height: 3,
                      decoration: BoxDecoration(
                          color: index >= i ? accent : line,
                          borderRadius: BorderRadius.circular(2))),
                  const SizedBox(height: 7),
                  Text(tr(titles[i]),
                      textAlign: TextAlign.center,
                      style:
                          small.copyWith(color: index == i ? accent : muted)),
                ]))),
    ]);
  }

  Widget metric(String name, String value) =>
      Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(tr(name), style: small),
        const SizedBox(height: 4),
        Text(value,
            style: const TextStyle(
                fontSize: 18,
                fontWeight: FontWeight.w600,
                fontFeatures: [FontFeature.tabularFigures()]))
      ]);
  Widget body(Map flow) {
    final id = flow['id'] as String, current = stage(flow);
    final expanded = _expanded.contains(id), tab = _tabs[id] ?? 0;
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      phases(flow),
      const SizedBox(height: 20),
      Wrap(spacing: 32, runSpacing: 14, children: [
        metric('P2P round', number(flow['round'])),
        metric('Total wait', duration(flow['elapsed_ms'], clock: true)),
        if (current == 'backoff')
          metric('Next P2P attempt in', duration(flow['retry_ms']))
        else if (!['waiting', 'slot', 'relay', 'cancelling'].contains(current))
          metric('Round time',
              '${duration(flow['round_ms'])} / ${duration(flow['limit_ms'])}'),
      ]),
      if (flow['reason'] != null)
        Padding(
            padding: const EdgeInsets.only(top: 16),
            child: card(
                Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(tr('Last P2P result'), style: small),
                  const SizedBox(height: 4),
                  Text(reason(flow['reason'])),
                ]),
                tint: current == 'waiting'
                    ? Colors.amber.withOpacity(.07)
                    : null)),
      const SizedBox(height: 16),
      pair(
          networkSummary(
              'Local network', flow['local'], Icons.computer_outlined),
          networkSummary('Peer network', flow['peer'], Icons.devices_outlined)),
      const SizedBox(height: 12),
      Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Icon(Icons.route_outlined, size: 18, color: accent),
        const SizedBox(width: 8),
        Expanded(
            child: Text('${tr('Strategy')}: ${mode(data(flow['strategy']))}',
                style: small)),
      ]),
      const SizedBox(height: 8),
      Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
              key: ValueKey('details-$id'),
              style: TextButton.styleFrom(foregroundColor: accent),
              icon: Icon(expanded ? Icons.expand_less : Icons.expand_more,
                  size: 18),
              label:
                  Text(tr(expanded ? 'Hide P2P details' : 'Show P2P details')),
              onPressed: () => setState(() {
                    expanded ? _expanded.remove(id) : _expanded.add(id);
                  }))),
      if (expanded) ...[
        const Divider(height: 20),
        Wrap(spacing: 8, runSpacing: 8, children: [
          for (var i = 0; i < 3; i++)
            ChoiceChip(
                key: ValueKey('tab-$id-$i'),
                label: Text(tr(['Network', 'Strategy', 'History'][i])),
                selected: tab == i,
                selectedColor: accent.withOpacity(.15),
                backgroundColor: Colors.transparent,
                shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(8),
                    side: BorderSide(
                        color: tab == i ? accent.withOpacity(.4) : line)),
                labelStyle: TextStyle(
                    fontSize: 13,
                    color: tab == i
                        ? accent
                        : Theme.of(context).colorScheme.onSurface,
                    fontWeight: FontWeight.w600),
                onSelected: (_) => setState(() => _tabs[id] = i)),
        ]),
        const SizedBox(height: 16),
        if (tab == 0)
          pair(networkDetails('Local network', flow['local']),
              networkDetails('Peer network', flow['peer'])),
        if (tab == 1) strategyDetails(flow),
        if (tab == 2) history(flow),
      ],
      if (_readError != null)
        Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(_readError!, style: small)),
    ]);
  }

  Widget header(Map? flow, {bool active = true}) {
    final current = flow == null ? '' : stage(flow);
    final parked = current == 'waiting',
        backoff = current == 'backoff',
        verified = current == 'authenticated';
    final title = parked
        ? 'Waiting for connection choice'
        : backoff
            ? 'Waiting to retry P2P'
            : current == 'relay'
                ? 'Relay connection'
                : current == 'cancelling'
                    ? 'Stopping P2P'
                    : verified
                        ? 'Direct channel ready'
                        : active
                            ? 'Trying P2P first'
                            : 'Connecting...';
    final detail = verified
        ? tr('Continuing with the original login process')
        : current.isEmpty
            ? null
            : stageText(current);
    return Padding(
        padding: EdgeInsets.all(widget.embedded ? 12 : 20),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                  color: (parked ? Colors.amber : accent).withOpacity(.10),
                  borderRadius: BorderRadius.circular(10)),
              child: Center(
                  child: parked || backoff || verified
                      ? Icon(
                          parked
                              ? Icons.info_outline
                              : backoff
                                  ? Icons.schedule
                                  : Icons.check,
                          color: parked ? Colors.amber.shade700 : accent,
                          size: 20)
                      : SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(
                              strokeWidth: 2, color: accent)))),
          const SizedBox(width: 12),
          Expanded(
              child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                Text(tr(title),
                    style: const TextStyle(
                        fontSize: 18,
                        fontWeight: FontWeight.w600,
                        height: 1.35)),
                if (detail != null && detail != tr(title))
                  Padding(
                      padding: const EdgeInsets.only(top: 4),
                      child: Text(detail, style: small)),
                if (flow?['mapping'] != null)
                  Text('${tr('Local Port')}: ${flow!['mapping']}',
                      style: small),
              ])),
        ]));
  }

  Widget actions(Map? flow, {required bool active}) {
    final current = flow == null ? '' : stage(flow);
    final showRelay =
        active || flow == null || current == 'relay' || current == 'cancelling';
    final canRelay = active &&
        !['relay', 'cancelling', 'authenticated'].contains(current) &&
        !_submitted.containsValue('relay');
    final style = OutlinedButton.styleFrom(
        minimumSize: const Size(64, 44),
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        foregroundColor: accent,
        disabledForegroundColor: muted.withOpacity(.5),
        side: BorderSide(color: canRelay ? accent.withOpacity(.6) : line),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)));
    final cancel = TextButton(
        key: ValueKey('cancel-${flow?['id'] ?? 'connecting'}'),
        style: TextButton.styleFrom(
            minimumSize: const Size(64, 44),
            foregroundColor: Theme.of(context).colorScheme.onSurface,
            disabledForegroundColor: muted.withOpacity(.5)),
        onPressed: current == 'cancelling'
            ? null
            : flow != null && active
                ? () => _request(flow, 'cancel')
                : widget.onCancel,
        child: Text(tr('Cancel')));
    final relay = current == 'waiting'
        ? ElevatedButton(
            key: ValueKey('relay-${flow?['id']}'),
            style: ElevatedButton.styleFrom(
                backgroundColor: accent,
                foregroundColor: Colors.white,
                minimumSize: const Size(64, 44),
                padding:
                    const EdgeInsets.symmetric(horizontal: 16, vertical: 10)),
            onPressed: canRelay ? () => _request(flow!, 'relay') : null,
            child: Text(tr('Relay connection')))
        : OutlinedButton(
            key: ValueKey('relay-${flow?['id']}'),
            style: style,
            onPressed: canRelay ? () => _request(flow!, 'relay') : null,
            child: Text(tr('Relay connection')));
    final copy = flow == null
        ? const SizedBox.shrink()
        : IconButton(
            key: ValueKey('copy-${flow['id']}'),
            tooltip:
                tr(_copied == flow['id'] ? 'Copied' : 'Copy P2P diagnostics'),
            onPressed: () => _copy(flow),
            icon: Icon(
                _copied == flow['id'] ? Icons.check : Icons.copy_outlined,
                size: 19),
            color: _copied == flow['id'] ? accent : muted);
    return Padding(
        padding: EdgeInsets.symmetric(
            horizontal: widget.embedded ? 12 : 20, vertical: 10),
        child: LayoutBuilder(builder: (_, constraints) {
          if (constraints.maxWidth < 350 &&
              MediaQuery.textScalerOf(context).scale(14) > 20) {
            return Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (flow != null)
                    Align(alignment: Alignment.centerLeft, child: copy),
                  cancel,
                  if (showRelay) relay
                ]);
          }
          return Row(children: [
            copy,
            const SizedBox(width: 8),
            Expanded(
                child: Wrap(
                    alignment: WrapAlignment.end,
                    spacing: 8,
                    runSpacing: 8,
                    children: [cancel, if (showRelay) relay])),
          ]);
        }));
  }

  Widget panel(Map? flow, {required bool active}) {
    final id = flow?['id'] as String? ?? 'connecting';
    final controller = _scroll.putIfAbsent(id, () => ScrollController());
    return Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          header(flow, active: active),
          Divider(height: 1, color: line),
          if (active)
            Expanded(
                child: Scrollbar(
                    controller: controller,
                    child: SingleChildScrollView(
                        key: PageStorageKey('body-$id'),
                        controller: controller,
                        padding: const EdgeInsets.all(20),
                        child: body(flow!))))
          else
            Expanded(
                child: Padding(
                    padding: const EdgeInsets.all(20),
                    child: Text(
                        _readError ?? tr('Connection status will appear here'),
                        style: small))),
          Divider(height: 1, color: line),
          actions(flow, active: active),
        ]);
  }

  Widget embedded(Map flow) {
    final id = flow['id'] as String;
    return Container(
        margin: const EdgeInsets.only(bottom: 8),
        decoration: BoxDecoration(
            border: Border.all(color: line),
            borderRadius: BorderRadius.circular(10)),
        child:
            Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          header(flow),
          actions(flow, active: true),
          Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: Row(children: [
                Expanded(
                    child: Text(
                        '${tr('P2P round')} ${number(flow['round'])} · ${duration(flow['elapsed_ms'], clock: true)}',
                        style: small)),
                IconButton(
                    key: ValueKey('embedded-details-$id'),
                    tooltip: tr('Show P2P details'),
                    onPressed: () => setState(() {
                          _expanded.contains(id)
                              ? _expanded.remove(id)
                              : _expanded.add(id);
                        }),
                    icon: Icon(_expanded.contains(id)
                        ? Icons.expand_less
                        : Icons.expand_more)),
              ])),
          if (_expanded.contains(id))
            Padding(padding: const EdgeInsets.all(12), child: body(flow)),
        ]));
  }

  @override
  Widget build(BuildContext context) {
    final active = _snapshots.where((flow) => flow['active'] == true).toList();
    final last = _snapshots.isEmpty ? null : _snapshots.last;
    return VisibilityDetector(
        key: ValueKey(this),
        onVisibilityChanged: (info) {
          final visible = info.visibleFraction > 0;
          if (_visible != visible) {
            _visible = visible;
            _refreshTimer();
          }
        },
        child: DefaultTextStyle.merge(
            style: TextStyle(
                fontSize: 14,
                height: 1.4,
                color: Theme.of(context).colorScheme.onSurface),
            child: ConstrainedBox(
                constraints: BoxConstraints(
                    maxWidth: 720,
                    maxHeight: widget.embedded
                        ? 320
                        : (MediaQuery.sizeOf(context).height -
                                MediaQuery.paddingOf(context).vertical) *
                            .85),
                child: widget.embedded
                    ? SingleChildScrollView(
                        child: Column(
                            mainAxisSize: MainAxisSize.min,
                            children: active.map(embedded).toList()))
                    : SizedBox(
                        width: 720,
                        height: 640,
                        child: panel(active.isEmpty ? last : active.last,
                            active: active.isNotEmpty)))));
  }
}
