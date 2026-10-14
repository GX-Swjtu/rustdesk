import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart' hide Dialog;
import 'package:flutter_hbb/common/widgets/native_p2p_waiting.dart';
import 'package:flutter_hbb/models/platform_model.dart';

String nativeP2pDialogTag(SessionID id) => '$id-native-p2p';

Widget nativeP2pPanel(SessionID id,
        {VoidCallback? onCancel, bool embedded = false}) =>
    NativeP2pWaiting(
        readSnapshot: () => bind.sessionNativeP2PSnapshot(sessionId: id),
        control: (flow, action) => bind.sessionNativeP2PControl(
            sessionId: id, flowId: flow, action: action),
        translate: (key) => translate(key),
        onCancel: onCancel,
        embedded: embedded);

extension NativeP2pDialogs on OverlayDialogManager {
  String showConnectionWaiting(SessionID sessionId, {VoidCallback? onCancel}) {
    if (isWeb || !(isWindows || isLinux || isAndroid)) {
      return showLoading(translate('Connecting...'), onCancel: onCancel);
    }
    final tag = nativeP2pDialogTag(sessionId);
    dismissByTag(tag);
    show(
        (setState, close, context) => NativeP2pDialog(
            content: nativeP2pPanel(sessionId, onCancel: onCancel),
            onCancel: onCancel),
        tag: tag);
    return tag;
  }
}

// The overlay manager expects CustomAlertDialog. Keep this layout local to P2P
// instead of changing the scrolling and sizing of every application dialog.
class NativeP2pDialog extends CustomAlertDialog {
  const NativeP2pDialog({required super.content, super.onCancel});

  @override
  Widget build(BuildContext context) => CallbackShortcuts(
          bindings: {
            const SingleActivator(LogicalKeyboardKey.escape): () =>
                onCancel?.call()
          },
          child: FocusScope(
              autofocus: true,
              child: SafeArea(
                  child: Dialog(
                      insetPadding: const EdgeInsets.symmetric(
                          horizontal: 16, vertical: 24),
                      clipBehavior: Clip.antiAlias,
                      shape: RoundedRectangleBorder(
                          borderRadius: BorderRadius.circular(16)),
                      child: content))));
}
