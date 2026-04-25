// Desktop shell — the top-level layout for tablet/desktop/wideDesktop
// breakpoints. Hosts the chrome (titlebar, future sidebar) and a
// content slot for whatever screen the user is on.
//
// Pre-Phase 6: this is a thin scaffold so the pair-mobile titlebar
// button is reachable end-to-end. The real sidebar + project page
// land in Phase 3 #2; the placeholder "Welcome" body keeps the
// shell shippable in the meantime.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/iroh_client.dart';
import '../state/local_connection_provider.dart';
import '../tokens.dart';
import 'pair_mobile/pair_mobile_modal.dart';

const double _titlebarHeight = 32;
const double _sidebarWidth = 280;

class DesktopShell extends ConsumerWidget {
  const DesktopShell({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Eagerly read the local connection so the daemon-backed
    // transport spins up before anything tries to render projects.
    ref.watch(localConnectionProvider);
    return Scaffold(
      backgroundColor: AppTokens.terminalBg,
      body: Column(
        children: [
          const _Titlebar(),
          Expanded(
            child: Row(
              children: [
                const _Sidebar(),
                Expanded(child: _MainArea()),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _Sidebar extends ConsumerWidget {
  const _Sidebar();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final projects = ref.watch(desktopProjectsProvider);
    return Container(
      width: _sidebarWidth,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          right: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: projects.when(
        data: _ProjectList.new,
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => _SidebarMessage(text: 'Project list error: $e'),
      ),
    );
  }
}

class _ProjectList extends StatelessWidget {
  const _ProjectList(this.projects);

  final List<ProjectSummary> projects;

  @override
  Widget build(BuildContext context) {
    if (projects.isEmpty) {
      return const _SidebarMessage(
        text: 'No projects yet.\nAdd one from the desktop app to see it here.',
      );
    }
    return ListView.builder(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.space3),
      itemCount: projects.length,
      itemBuilder: (_, i) => _ProjectRow(projects[i]),
    );
  }
}

class _ProjectRow extends StatelessWidget {
  const _ProjectRow(this.project);

  final ProjectSummary project;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space5,
        vertical: AppTokens.space1,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                width: 14,
                height: 14,
                decoration: BoxDecoration(
                  color: AppTokens.projectColor(project.id),
                  borderRadius: BorderRadius.circular(AppTokens.radiusXs),
                ),
              ),
              const SizedBox(width: AppTokens.space3),
              Expanded(
                child: Text(
                  project.name,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: AppTokens.fontBodyLg,
                    color: AppTokens.textPrimary,
                    fontWeight: FontWeight.w500,
                  ),
                ),
              ),
            ],
          ),
          if (project.tasks.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(
                left: 22,
                top: AppTokens.space1,
                bottom: AppTokens.space2,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final task in project.tasks)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 1),
                      child: Text(
                        task.name,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: AppTokens.fontBody,
                          color: AppTokens.textSecondary,
                        ),
                      ),
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

class _SidebarMessage extends StatelessWidget {
  const _SidebarMessage({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(AppTokens.space7),
      child: Text(
        text,
        style: const TextStyle(
          fontSize: AppTokens.fontBody,
          color: AppTokens.textMuted,
        ),
      ),
    );
  }
}

class _MainArea extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return Container(
      color: AppTokens.terminalBg,
      alignment: Alignment.center,
      child: const _WelcomePlaceholder(),
    );
  }
}

class _Titlebar extends StatelessWidget {
  const _Titlebar();

  @override
  Widget build(BuildContext context) {
    return Container(
      height: _titlebarHeight,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          bottom: BorderSide(color: AppTokens.divider, width: 0.5),
        ),
      ),
      child: Row(
        children: [
          const SizedBox(width: AppTokens.space5),
          const Text(
            'AnotherOne',
            style: TextStyle(
              fontSize: AppTokens.fontBody,
              fontWeight: FontWeight.w500,
              color: AppTokens.textSecondary,
            ),
          ),
          const Spacer(),
          const _PairMobileButton(),
          const SizedBox(width: AppTokens.space2),
        ],
      ),
    );
  }
}

class _PairMobileButton extends StatelessWidget {
  const _PairMobileButton();

  @override
  Widget build(BuildContext context) {
    return _TitlebarIconButton(
      tooltip: 'Pair a mobile device with the embedded daemon',
      icon: Icons.qr_code_2,
      onPressed: () => showPairMobileModal(context),
    );
  }
}

class _TitlebarIconButton extends StatefulWidget {
  const _TitlebarIconButton({
    required this.tooltip,
    required this.icon,
    required this.onPressed,
  });

  final String tooltip;
  final IconData icon;
  final VoidCallback onPressed;

  @override
  State<_TitlebarIconButton> createState() => _TitlebarIconButtonState();
}

class _TitlebarIconButtonState extends State<_TitlebarIconButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            width: 40,
            height: 24,
            decoration: BoxDecoration(
              color: _hovered ? AppTokens.overlayHoverStrong : AppTokens.overlayRest,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
              border: Border.all(color: AppTokens.border),
            ),
            alignment: Alignment.center,
            child: Icon(
              widget.icon,
              size: AppTokens.iconSizeDefault,
              color: AppTokens.textPrimary,
            ),
          ),
        ),
      ),
    );
  }
}

class _WelcomePlaceholder extends StatelessWidget {
  const _WelcomePlaceholder();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(AppTokens.space10),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(
            Icons.terminal,
            size: 64,
            color: AppTokens.textMuted,
          ),
          const SizedBox(height: AppTokens.space5),
          const Text(
            'AnotherOne',
            style: TextStyle(
              fontSize: AppTokens.fontHeadingLg,
              fontWeight: FontWeight.w600,
              color: AppTokens.textPrimary,
            ),
          ),
          const SizedBox(height: AppTokens.space2),
          const Text(
            'Desktop UI under construction. The sidebar, project view,\n'
            'and task panes land in subsequent phases.',
            textAlign: TextAlign.center,
            style: TextStyle(
              fontSize: AppTokens.fontBodyLg,
              color: AppTokens.textMuted,
            ),
          ),
          const SizedBox(height: AppTokens.space7),
          Text(
            'Use the QR button above to pair a mobile device.',
            style: TextStyle(
              fontSize: AppTokens.fontBody,
              color: AppTokens.textPlaceholder,
            ),
          ),
        ],
      ),
    );
  }
}
