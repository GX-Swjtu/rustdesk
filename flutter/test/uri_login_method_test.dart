import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_test/flutter_test.dart' hide isLinux;

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('cold-start link credentials stay out of logs', () async {
    const channel = MethodChannel('uni_links/messages');
    const marker = 'test-link-secret';
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(
        channel, (_) async => 'rustdesk://x/unused?password=$marker');
    addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
    final messages = <String>[];
    await runZoned(initUniLinks, zoneSpecification: ZoneSpecification(
      print: (_, __, ___, message) => messages.add(message),
    ));
    expect(messages.join('\n'), isNot(contains(marker)));
  }, skip: isLinux || isWeb); // Linux uses D-Bus to receive links.

  test('request-approval links preserve their connection type', () {
    for (final type in [
      'connect',
      'play',
      'file-transfer',
      'view-camera',
      'port-forward',
      'rdp',
      'terminal',
      'terminal-admin',
    ]) {
      expect(
        urlLinkToCmdArgs(
            Uri.parse('rustdesk://$type/123456789?request-approval=true')),
        ['--$type', '123456789', '--request-approval'],
      );
    }
    expect(
      urlLinkToCmdArgs(Uri.parse('rustdesk://123456789?request-approval=true')),
      ['--connect', '123456789', '--request-approval'],
    );
  });

  test('omitted and empty passwords retain automatic login', () {
    expect(urlLinkToCmdArgs(Uri.parse('rustdesk://123456789')),
        ['--connect', '123456789']);
    for (final query in ['password=', 'password=&request-approval=false']) {
      expect(urlLinkToCmdArgs(Uri.parse('rustdesk://123456789?$query')),
          ['--connect', '123456789', '--password', '']);
    }
  });

  test('request-approval ignores an explicit password in either order', () {
    for (final query in [
      'password=provided&request-approval=true',
      'request-approval=true&password=provided',
    ]) {
      expect(urlLinkToCmdArgs(Uri.parse('rustdesk://123456789?$query')),
          ['--connect', '123456789', '--request-approval']);
    }
  });
}
