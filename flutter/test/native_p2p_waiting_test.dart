import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:visibility_detector/visibility_detector.dart';
import 'package:flutter_hbb/common/widgets/native_p2p_waiting.dart';

Map<String, dynamic> flow({String stage = 'waiting'}) => {
      'id': '1',
      'active': true,
      'round': 4,
      'stage': stage,
      'elapsed_ms': 170000,
      'round_ms': 12000,
      'limit_ms': 60000,
      'retry_ms': stage == 'backoff' ? 5000 : null,
      'reason': 'No applicable strategy',
      'local': null,
      'peer': null,
      'strategy': null,
      'rounds': [],
      'events': [],
    };

Widget page(
        {required Future<String> Function() read,
        Future<bool> Function(String, String)? control}) =>
    MaterialApp(
        home: Scaffold(
            body: Center(
                child: NativeP2pWaiting(
                    readSnapshot: read,
                    control: control ?? (_, __) async => true,
                    translate: (key) => key))));

void main() {
  setUp(() {
    VisibilityDetectorController.instance.updateInterval = Duration.zero;
  });

  testWidgets('no strategy stops spinner, narrow layout and secret-free copy',
      (tester) async {
    tester.view.physicalSize = const Size(320, 640);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    String? copied;
    tester.binding.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData')
        copied = (call.arguments as Map)['text'];
      return null;
    });
    await tester.pumpWidget(page(read: () async => jsonEncode([flow()])));
    await tester.pump();
    expect(find.byType(CircularProgressIndicator), findsNothing);
    expect(find.text('Waiting for connection choice'), findsWidgets);
    await tester.tap(find.text('Show P2P details'));
    await tester.pump();
    expect(find.textContaining('Unknown'), findsWidgets);
    final copy = find.text('Copy P2P diagnostics');
    await tester.ensureVisible(copy);
    await tester.tap(copy);
    await tester.pump();
    expect(jsonDecode(copied!)['round'], 4);
    expect(copied, isNot(contains('secret')));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
      'only one snapshot request in flight and background polling stops',
      (tester) async {
    final pending = Completer<String>();
    var reads = 0;
    await tester.pumpWidget(page(read: () {
      reads++;
      return pending.future;
    }));
    await tester.pump(const Duration(seconds: 3));
    expect(reads, 1);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    pending.complete(jsonEncode([flow(stage: 'backoff')]));
    await tester.pump(const Duration(seconds: 3));
    expect(reads, 1);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(reads, 2);
    expect(find.textContaining('Next P2P attempt in'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await tester.pump(const Duration(seconds: 2));
    expect(reads, 2);
  });

  testWidgets('relay button submits once while cleanup is pending',
      (tester) async {
    final accepted = Completer<bool>();
    var requests = 0;
    await tester.pumpWidget(page(
        read: () async => jsonEncode([flow(stage: 'probe')]),
        control: (id, action) {
          expect(id, '1');
          expect(action, 'relay');
          requests++;
          return accepted.future;
        }));
    await tester.pump();
    await tester.tap(find.text('Relay connection'));
    await tester.pump();
    await tester.tap(find.text('Relay connection'));
    expect(requests, 1);
    expect(
        find.text('Stopping P2P and connecting through relay'), findsOneWidget);
    accepted.complete(true);
    await tester.pump();
    await tester.pumpWidget(const SizedBox());
  });
}
