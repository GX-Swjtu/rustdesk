import 'package:flutter/material.dart';
import 'package:get/get.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../common.dart';
import '../../models/platform_model.dart';
import '../../models/state_model.dart';

class ForkUpdateCard extends StatelessWidget {
  const ForkUpdateCard({super.key});

  @override
  Widget build(BuildContext context) {
    return Obx(() {
      final status = stateGlobal.forkUpdate;
      final available = status['available'] as String? ?? '';
      final url = status['url'] as String? ?? '';
      final upstream = status['upstream'] as String? ?? '';
      if (status['upstream_newer'] != true &&
          available.isEmpty &&
          status['error'] != true) {
        return const SizedBox.shrink();
      }
      return Card(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (status['upstream_newer'] == true)
                Text('${translate('New upstream release')}: $upstream'),
              Text(available.isEmpty
                  ? translate(status['error'] == true
                      ? 'Release check unavailable. Please try again later.'
                      : 'No published fork update is available for this platform.')
                  : '${translate('Fork update available')}: $available'),
              if (available.isNotEmpty && url.isNotEmpty)
                TextButton(
                  onPressed: () => launchUrl(Uri.parse(url),
                      mode: LaunchMode.externalApplication),
                  child: Text(translate('Download fork release')),
                ),
              TextButton(
                onPressed: () => bind.mainGetSoftwareUpdateUrl(),
                child: Text(translate('Check for updates')),
              ),
            ],
          ),
        ),
      );
    });
  }
}
