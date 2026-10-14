import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:visibility_detector/visibility_detector.dart';
import 'package:flutter_hbb/common/native_p2p_dialog.dart';
import 'package:flutter_hbb/common/widgets/native_p2p_waiting.dart';

Map<String, dynamic> network(
        {bool stable = false,
        int samples = 4,
        bool? predictable = false,
        bool multiple = false}) =>
    {
      'candidates': {
        'local': ['192.168.2.10:50960'],
        'public': List.generate(
            samples,
            (i) =>
                '${multiple && i.isOdd ? '198.51.100.25' : '203.0.113.10'}:${stable ? 41535 : 3464 + i * 3}'),
      },
      'profile': {
        'easy': stable,
        'predictable': predictable,
        'ips': ['203.0.113.10', if (multiple) '198.51.100.25']
      },
    };

Map<String, dynamic> flow({String id = '1', String stage = 'probe'}) => {
      'id': id,
      'active': true,
      'round': 4,
      'stage': stage,
      'elapsed_ms': 170000,
      'round_ms': 12000,
      'limit_ms': 45000,
      'retry_ms': stage == 'backoff' ? 5000 : null,
      'reason': null,
      'local': network(),
      'peer': network(stable: true),
      'strategy': {
        'behavior': {'mode': 'Birthday', 'ttl': 7, 'delay_ms': 3000},
        'sender': false
      },
      'probes': {'sent': 1025, 'authenticated': 0, 'sockets': 257},
      'rounds': [],
      'events': [],
    };

final captureKey = GlobalKey();
Widget page(
        {required Future<String> Function() read,
        Future<bool> Function(String, String)? control,
        VoidCallback? onCancel,
        bool embedded = false,
        Brightness brightness = Brightness.dark,
        double scale = 1,
        String Function(String)? translate}) =>
    MaterialApp(
        theme: ThemeData(
            brightness: brightness,
            useMaterial3: false,
            fontFamily: 'PanelTest',
            colorScheme: ColorScheme.fromSeed(
                    seedColor: const Color(0xff007bff), brightness: brightness)
                .copyWith(primary: const Color(0xff007bff)),
            scaffoldBackgroundColor: brightness == Brightness.dark
                ? const Color(0xff101114)
                : const Color(0xfff1f3f6),
            dialogBackgroundColor: brightness == Brightness.dark
                ? const Color(0xff18191e)
                : Colors.white,
            cardColor: brightness == Brightness.dark
                ? const Color(0xff202127)
                : const Color(0xfff7f8fa)),
        home: Builder(
            builder: (context) => MediaQuery(
                data:
                    MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
                child: RepaintBoundary(key: captureKey, child: Scaffold(body: Center(child: NativeP2pDialog(onCancel: onCancel, content: NativeP2pWaiting(readSnapshot: read, control: control ?? (_, __) async => true, translate: translate ?? (key) => key, onCancel: onCancel, embedded: embedded))))))));

Future<void> press(WidgetTester tester, String key) async {
  final target = find.byKey(ValueKey(key));
  await tester.ensureVisible(target);
  await tester.pump();
  await tester.tap(target);
  await tester.pump();
}

Future<void> screenshot(WidgetTester tester, String name) async {
  const destination = String.fromEnvironment('P2P_SCREENSHOT_DIR');
  if (destination.isEmpty) return;
  await tester.pump(const Duration(milliseconds: 180));
  final boundary =
      captureKey.currentContext!.findRenderObject() as RenderRepaintBoundary;
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: 1);
    final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
    await Directory(destination).create(recursive: true);
    await File('$destination/$name.png')
        .writeAsBytes(bytes!.buffer.asUint8List());
    image.dispose();
  });
}

void main() {
  setUp(() =>
      VisibilityDetectorController.instance.updateInterval = Duration.zero);

  testWidgets('stages, retry, parked errors, diagnostics and delivered channel',
      (tester) async {
    var current = flow(stage: 'prepare');
    String? copied;
    tester.binding.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') {
        copied = (call.arguments as Map)['text'];
      }
      return null;
    });
    await tester.pumpWidget(page(read: () async => jsonEncode([current])));
    await tester.pump();
    expect(find.text('Preparing P2P session'), findsOneWidget);
    final relayPosition = tester.getRect(find.byKey(const ValueKey('relay-1')));
    expect(find.text('12 s / 45 s'),
        findsOneWidget); // Uses the ticket deadline, not a hard-coded 60.
    for (final entry in {
      'network': 'Discovering UDP mappings',
      'peer': 'Waiting for peer candidates',
      'probe': 'Probing P2P strategy',
      'kcp': 'Establishing KCP',
      'secure': 'Verifying device identity'
    }.entries) {
      current['stage'] = entry.key;
      await tester.pump(const Duration(milliseconds: 500));
      expect(find.text(entry.value), findsOneWidget);
    }
    current = flow(stage: 'backoff')..['reason'] = 'native coordinator closed';
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Next P2P attempt in'), findsOneWidget);
    expect(find.text('5 s'), findsOneWidget);
    expect(
        tester.getRect(find.byKey(const ValueKey('relay-1'))), relayPosition);
    expect(find.text('The coordination server closed the connection'),
        findsOneWidget);
    current['stage'] = 'waiting';
    current['reason'] = 'Unclassified failure ${'long-message ' * 100}';
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.byType(CircularProgressIndicator), findsNothing);
    expect(find.text('Waiting for connection choice'), findsOneWidget);
    expect(find.byKey(const ValueKey('relay-1')), findsOneWidget);
    expect(tester.widget(find.byKey(const ValueKey('relay-1'))),
        isA<ElevatedButton>());
    expect(find.text(current['reason']), findsNothing);
    await press(tester, 'details-1');
    await press(tester, 'tab-1-2');
    await tester.ensureVisible(find.text('Technical message').first);
    await tester.tap(find.text('Technical message').first);
    await tester.pump(const Duration(milliseconds: 250));
    expect(find.text(current['reason']), findsOneWidget);
    await press(tester, 'copy-1');
    expect(jsonDecode(copied!), current);
    expect(find.byTooltip('Copied'), findsOneWidget);
    current = flow(stage: 'relay')..['active'] = false;
    await tester.pump(const Duration(milliseconds: 500));
    // Keep the disabled relay slot; a second click must not hit a moved Cancel.
    expect(
        tester
            .widget<OutlinedButton>(find.byKey(const ValueKey('relay-1')))
            .onPressed,
        isNull);
    expect(
        tester.getRect(find.byKey(const ValueKey('relay-1'))), relayPosition);
    current = flow(stage: 'authenticated')..['active'] = false;
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Direct channel ready'), findsOneWidget);
    expect(find.text('Continuing with the original login process'),
        findsOneWidget);
    expect(find.byKey(const ValueKey('relay-1')), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'dialog traps keyboard traversal and preserves Escape cancellation',
      (tester) async {
    final behind = FocusNode(), first = FocusNode(), last = FocusNode();
    addTearDown(behind.dispose);
    addTearDown(first.dispose);
    addTearDown(last.dispose);
    var cancelled = 0;
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: Stack(children: [
      TextButton(
          focusNode: behind, onPressed: () {}, child: const Text('Behind')),
      NativeP2pDialog(
          onCancel: () => cancelled++,
          content: Row(mainAxisSize: MainAxisSize.min, children: [
            TextButton(
                focusNode: first, onPressed: () {}, child: const Text('First')),
            TextButton(
                focusNode: last, onPressed: () {}, child: const Text('Last')),
          ])),
    ]))));
    last.requestFocus();
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await tester.pump();
    expect(first.hasFocus, isTrue);
    expect(behind.hasFocus, isFalse);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.pump();
    expect(last.hasFocus, isTrue);
    expect(behind.hasFocus, isFalse);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    expect(cancelled, 1);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'history distinguishes connection choices from stages and missing records',
      (tester) async {
    final current = flow()
      ..['events'] = [
        {'ms': 100, 'round': 4, 'stage': 'secure'},
        {'ms': 200, 'round': 4, 'choice': 'Direct'},
        {'ms': 300, 'round': 4, 'choice': 'Relay'},
        {'ms': 400, 'round': 4, 'choice': 'Cancel'},
        {'ms': 500, 'round': 4, 'choice': 'Legacy'},
        {'ms': 600, 'round': 4},
      ];
    await tester.pumpWidget(page(read: () async => jsonEncode([current])));
    await tester.pump();
    await press(tester, 'details-1');
    await press(tester, 'tab-1-2');
    expect(find.text('Verifying device identity'), findsOneWidget);
    expect(find.text('Direct channel ready'), findsOneWidget);
    expect(find.text('Relay connection'), findsNWidgets(2));
    expect(find.text('Stopping P2P'), findsOneWidget);
    expect(find.text('Connecting...'), findsOneWidget);
    expect(find.text('Unknown'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'network evidence, unknowns, structured strategy and reading position',
      (tester) async {
    var current = flow();
    await tester.pumpWidget(page(read: () async => jsonEncode([current])));
    await tester.pump();
    expect(find.text('Port prediction is not needed'), findsOneWidget);
    expect(find.text('No predictable port pattern observed'), findsOneWidget);
    current['local'] = network(samples: 2);
    await tester.pump(const Duration(milliseconds: 500));
    expect(
        find.text('Insufficient evidence for port prediction'), findsOneWidget);
    current['local'] = network(
        multiple: true); // Two per exit is not three observations of one exit.
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Multiple public exits'), findsOneWidget);
    expect(
        find.text('Insufficient evidence for port prediction'), findsOneWidget);
    current['local'] = network(predictable: true);
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Predictable port'), findsOneWidget);
    await press(tester, 'details-1');
    await press(tester, 'tab-1-0');
    expect(find.text('203.0.113.10:3464'), findsOneWidget);
    await press(tester, 'tab-1-1');
    current['strategy']['behavior']['ttl'] = 0;
    current['probes'] = null;
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('System default'), findsOneWidget);
    expect(find.text('0 / 8192'), findsNothing);
    expect(find.text('0 / 257'), findsNothing);
    final scroll = tester
        .widget<SingleChildScrollView>(
            find.byKey(const PageStorageKey('body-1')))
        .controller!;
    scroll.jumpTo(150);
    await tester.pump();
    final offset = scroll.offset;
    await tester.pump(const Duration(seconds: 2));
    expect(scroll.offset, offset);
    expect(
        tester
            .widget<ChoiceChip>(find.byKey(const ValueKey('tab-1-1')))
            .selected,
        isTrue);
    current['local'] = null;
    current['peer'] = null;
    current['strategy'] = null;
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Unknown'), findsWidgets);
    expect(find.textContaining('Public exits: 0'), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'one request in flight, pause, invisible, read failure and neutral startup',
      (tester) async {
    final pending = Completer<String>();
    var reads = 0, fail = false;
    var hidden = false;
    late StateSetter setVisibility;
    await tester.pumpWidget(StatefulBuilder(builder: (_, update) {
      setVisibility = update;
      return Offstage(
          offstage: hidden,
          child: page(read: () {
            reads++;
            if (fail) throw StateError('diagnostic read failed');
            return pending.future;
          }));
    }));
    expect(find.text('Connecting...'), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    expect(reads, 1);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    pending.complete(jsonEncode([flow(stage: 'backoff')]));
    await tester.pump(const Duration(seconds: 3));
    expect(reads, 1);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(reads, 2);
    fail = true;
    await tester.pump(const Duration(milliseconds: 500));
    expect(
        find.text('P2P diagnostics temporarily unavailable'), findsOneWidget);
    expect(find.text('Waiting to retry P2P'), findsOneWidget);
    setVisibility(() => hidden = true);
    await tester.pump();
    final whileHidden = reads;
    await tester.pump(const Duration(seconds: 2));
    expect(reads, whileHidden);
    setVisibility(() => hidden = false);
    await tester.pump();
    expect(reads, greaterThan(whileHidden));
    await tester.pumpWidget(const SizedBox());
    final finished = reads;
    await tester.pump(const Duration(seconds: 2));
    expect(reads, finished);
  });

  testWidgets(
      'relay submits once, shared-view cancel wins over late relay response',
      (tester) async {
    final accepted = Completer<bool>();
    var requests = 0, closes = 0;
    await tester.pumpWidget(page(
        read: () async => jsonEncode([flow()]),
        onCancel: () => closes++,
        control: (id, action) {
          expect(id, '1');
          expect(action, 'relay');
          requests++;
          return accepted.future;
        }));
    await tester.pump();
    await press(tester, 'relay-1');
    await press(tester, 'relay-1');
    expect(requests, 1);
    expect(
        find.text('Stopping P2P and connecting through relay'), findsOneWidget);
    await press(tester, 'cancel-1');
    expect(
        closes, 1); // Preserve the terminal's reference-counted close callback.
    accepted.complete(false);
    await tester.pump();
    expect(find.text('Stopping P2P'), findsOneWidget);
    expect(
        tester
            .widget<TextButton>(find.byKey(const ValueKey('cancel-1')))
            .onPressed,
        isNull);
    expect(requests, 1);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'embedded mappings keep independent tabs and only cancel their flow',
      (tester) async {
    final received = <String>[];
    final pending = Completer<bool>();
    final flows = [
      flow()..['mapping'] = 3389,
      flow(id: '2')..['mapping'] = 8080
    ];
    await tester.pumpWidget(page(
        embedded: true,
        read: () async => jsonEncode(flows),
        control: (id, action) {
          received.add('$id:$action');
          return pending.future;
        }));
    await tester.pump();
    await press(tester, 'embedded-details-2');
    await press(tester, 'tab-2-1');
    expect(find.byKey(const ValueKey('tab-1-1')), findsNothing);
    await press(tester, 'cancel-2');
    await press(tester, 'cancel-2');
    expect(received, ['2:cancel']);
    expect(
        tester
            .widget<TextButton>(find.byKey(const ValueKey('cancel-1')))
            .onPressed,
        isNotNull);
    pending.complete(true);
    await tester.pump();
    flows.removeLast();
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.byKey(const ValueKey('cancel-2')), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'responsive themes and text scaling keep footer accessible with long history',
      (tester) async {
    const font = String.fromEnvironment('P2P_TEST_FONT');
    const icons = String.fromEnvironment('P2P_TEST_ICON_FONT');
    const monospace = String.fromEnvironment('P2P_TEST_MONO_FONT');
    debugDisableShadows = false;
    addTearDown(() => debugDisableShadows = true);
    if (font.isNotEmpty) {
      await tester.runAsync(() async {
        await (FontLoader('PanelTest')
              ..addFont(File(font)
                  .readAsBytes()
                  .then((value) => ByteData.sublistView(value))))
            .load();
      });
    }
    if (icons.isNotEmpty) {
      await tester.runAsync(() async {
        await (FontLoader('MaterialIcons')
              ..addFont(File(icons)
                  .readAsBytes()
                  .then((value) => ByteData.sublistView(value))))
            .load();
      });
    }
    if (monospace.isNotEmpty) {
      await tester.runAsync(() async {
        await (FontLoader('monospace')
              ..addFont(File(monospace)
                  .readAsBytes()
                  .then((value) => ByteData.sublistView(value))))
            .load();
      });
    }
    final translations = <String, String>{};
    // Use the shipped locale, not an independently maintained screenshot translation.
    final source = File('../src/lang/cn.rs').readAsStringSync();
    for (final match
        in RegExp(r'\("([^"\\]+)", "((?:[^"\\]|\\.)*)"\)').allMatches(source)) {
      final text = jsonDecode('"${match[2]}"') as String;
      if (text.isNotEmpty) translations[match[1]!] = text;
    }
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    tester.view.devicePixelRatio = 1;
    final current = flow()..['reason'] = 'native coordinator closed';
    current['rounds'] = List.generate(
        20,
        (i) => {
              'round': i + 1,
              'elapsed_ms': 60000,
              'reason': 'Unclassified ${'long error ' * 60}',
              'strategy': current['strategy']
            });
    current['events'] = List.generate(
        200, (i) => {'round': i ~/ 6, 'ms': i * 1000, 'stage': 'probe'});
    for (final size in [const Size(1000, 800), const Size(320, 640)]) {
      for (final brightness in [Brightness.dark, Brightness.light]) {
        for (final scale in [1.0, 2.0]) {
          tester.view.physicalSize = size;
          await tester.pumpWidget(page(
              read: () async => jsonEncode([current]),
              brightness: brightness,
              scale: scale,
              translate: (key) => translations[key] ?? key));
          await tester.pump();
          final name =
              '${size.width.toInt()}-${brightness.name}-${scale.toInt()}x';
          await screenshot(tester, '$name-summary');
          await press(tester, 'details-1');
          await press(tester, 'tab-1-0');
          await screenshot(tester, '$name-network');
          await press(tester, 'tab-1-1');
          await screenshot(tester, '$name-strategy');
          await press(tester, 'tab-1-2');
          final scroll = tester
              .widget<SingleChildScrollView>(
                  find.byKey(const PageStorageKey('body-1')))
              .controller!;
          scroll.jumpTo(scroll.position.maxScrollExtent);
          await tester.pump();
          await screenshot(tester, '$name-history');
          for (final key in ['cancel-1', 'relay-1', 'copy-1']) {
            expect(find.byKey(ValueKey(key)).hitTestable(), findsOneWidget,
                reason: '$name / $key');
            expect(tester.getRect(find.byKey(ValueKey(key))).bottom,
                lessThanOrEqualTo(size.height));
          }
          expect(tester.takeException(), isNull, reason: name);
          await tester.pumpWidget(const SizedBox());
        }
      }
    }
    debugDisableShadows = true;
  });
}
