// Pixel-precise port of `desktop/src/titlebar.rs::titlebar_git_actions_button`
// + `titlebar_git_actions_overlay`. Split-button shape: primary
// half fires the dynamic primary action (Commit when there are
// unstaged changes, Push when ahead, Pull when behind, Fetch
// otherwise); chevron half toggles the dropdown.
//
// Lives as `part of desktop_titlebar.dart` so the chrome shell
// stays the single import surface.

part of 'desktop_titlebar.dart';

/// Each menu row's id maps 1:1 to the bridge action_id strings
/// recognised by `LocalSession::run_toolbar_git_action`.
enum _GitActionId {
  commit('commit'),
  commitAndPush('commit-and-push'),
  fetch('fetch'),
  pull('pull'),
  push('push'),
  forcePush('force-push'),
  createPr('create-pr'),
  createDraftPr('create-draft-pr');

  const _GitActionId(this.wireId);
  final String wireId;
}

class _PrimaryAction {
  const _PrimaryAction({
    required this.action,
    required this.label,
    required this.icon,
  });

  final _GitActionId action;
  final String label;
  final String icon;
}

/// Mirrors GPUI's `idle_titlebar_primary_git_action`: pick whichever
/// action the user is most likely to want next. Fetch is the
/// always-safe fallback when the working tree is clean and the
/// branch is in sync with its remote.
_PrimaryAction _computePrimaryAction({
  required bool hasChanges,
  required int aheadCount,
  required int behindCount,
}) {
  if (hasChanges) {
    return const _PrimaryAction(
      action: _GitActionId.commit,
      label: 'Commit',
      icon: 'git-commit',
    );
  }
  if (aheadCount > 0) {
    return _PrimaryAction(
      action: _GitActionId.push,
      label: _countLabel('Push', aheadCount),
      icon: 'cloud-upload',
    );
  }
  if (behindCount > 0) {
    return _PrimaryAction(
      action: _GitActionId.pull,
      label: _countLabel('Pull', behindCount),
      icon: 'git-pull',
    );
  }
  return const _PrimaryAction(
    action: _GitActionId.fetch,
    label: 'Fetch',
    icon: 'tool-download',
  );
}

String _countLabel(String base, int n) => n > 0 ? '$base ($n)' : base;

class _GitActionsButton extends ConsumerStatefulWidget {
  const _GitActionsButton();

  @override
  ConsumerState<_GitActionsButton> createState() => _GitActionsButtonState();
}

class _GitActionsButtonState extends ConsumerState<_GitActionsButton> {
  // Dimensions from desktop/src/titlebar.rs constants.
  static const double _buttonW = 156;
  static const double _buttonH = 28;
  static const double _chevronW = 26;
  static const double _menuW = 220;

  final OverlayPortalController _menu = OverlayPortalController();
  final LayerLink _link = LayerLink();
  bool _bodyHover = false;
  bool _chevronHover = false;
  bool _running = false;

  @override
  Widget build(BuildContext context) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) return const SizedBox.shrink();
    final files = ref.watch(changedFilesProvider(projectId)).valueOrNull;
    final gitState = ref.watch(activeGitStateProvider(projectId)).valueOrNull;
    final hasChanges = (files?.isNotEmpty) ?? false;
    final aheadCount = gitState?.aheadCount ?? 0;
    final behindCount = gitState?.behindCount ?? 0;
    final primary = _computePrimaryAction(
      hasChanges: hasChanges,
      aheadCount: aheadCount,
      behindCount: behindCount,
    );

    final menuOpen = _menu.isShowing;
    final containerBg = menuOpen
        ? const Color(0x1AFFFFFF) // white @ 0.10
        : const Color(0x0DFFFFFF); // white @ 0.05

    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: CompositedTransformTarget(
        link: _link,
        child: OverlayPortal(
          controller: _menu,
          overlayChildBuilder: (ctx) => _buildOverlay(
            ctx,
            projectId: projectId,
            hasChanges: hasChanges,
          ),
          child: Container(
            width: _buttonW,
            height: _buttonH,
            decoration: BoxDecoration(
              color: containerBg,
              borderRadius: BorderRadius.circular(11),
              border: Border.all(color: AppTokens.border),
            ),
            child: Row(
              children: [
                _buildPrimaryHalf(projectId, primary),
                _buildChevronHalf(projectId),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildPrimaryHalf(String projectId, _PrimaryAction primary) {
    final interactive = !_running;
    return Expanded(
      child: MouseRegion(
        cursor: interactive
            ? SystemMouseCursors.click
            : SystemMouseCursors.basic,
        onEnter: interactive ? (_) => setState(() => _bodyHover = true) : null,
        onExit: interactive ? (_) => setState(() => _bodyHover = false) : null,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: interactive ? () => _run(projectId, primary.action) : null,
          child: Container(
            decoration: BoxDecoration(
              color: interactive && _bodyHover
                  ? AppTokens.overlayHoverStrong
                  : Colors.transparent,
              border: const Border(
                right: BorderSide(color: AppTokens.divider),
              ),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 9),
            alignment: Alignment.centerLeft,
            child: Row(
              children: [
                if (_running)
                  const ToolbarSpinner(
                    size: 12,
                    color: Color(0xEBFFFFFF),
                  )
                else
                  AppIcon(
                    primary.icon,
                    size: 14,
                    color: const Color(0xEBFFFFFF), // white @ 0.92
                  ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    primary.label,
                    overflow: TextOverflow.ellipsis,
                    maxLines: 1,
                    style: const TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w500,
                      color: Color(0xDBFFFFFF), // white @ 0.86
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildChevronHalf(String projectId) {
    final interactive = !_running;
    return MouseRegion(
      cursor: interactive
          ? SystemMouseCursors.click
          : SystemMouseCursors.basic,
      onEnter: interactive ? (_) => setState(() => _chevronHover = true) : null,
      onExit: interactive ? (_) => setState(() => _chevronHover = false) : null,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: interactive
            ? () {
                setState(_menu.toggle);
                if (_menu.isShowing) {
                  // Refresh PR lookup on dropdown open — mirrors
                  // GPUI's refresh_active_project_pull_request_lookup.
                  ref.invalidate(pullRequestStatusProvider(projectId));
                }
              }
            : null,
        child: Container(
          width: _chevronW,
          height: _buttonH,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: interactive && _chevronHover
                ? AppTokens.overlayHoverStrong
                : Colors.transparent,
            borderRadius: const BorderRadius.only(
              topRight: Radius.circular(11),
              bottomRight: Radius.circular(11),
            ),
          ),
          child: const AppIcon(
            'chevron-down',
            size: 11,
            color: Color(0xADFFFFFF), // white @ 0.68
          ),
        ),
      ),
    );
  }

  Widget _buildOverlay(
    BuildContext context, {
    required String projectId,
    required bool hasChanges,
  }) {
    final pr = ref.watch(pullRequestStatusProvider(projectId));
    final hasExistingPr = pr.valueOrNull != null;
    final lookupChecked = pr.hasValue;
    final canCreatePr = !_running && lookupChecked && !hasExistingPr;
    return Stack(
      children: [
        Positioned.fill(
          child: GestureDetector(
            behavior: HitTestBehavior.translucent,
            onTap: () => setState(_menu.hide),
          ),
        ),
        CompositedTransformFollower(
          link: _link,
          targetAnchor: Alignment.bottomRight,
          followerAnchor: Alignment.topRight,
          offset: const Offset(0, 6),
          child: Material(
            color: Colors.transparent,
            child: Container(
              width: _menuW,
              decoration: BoxDecoration(
                color: AppTokens.cardBg,
                borderRadius: BorderRadius.circular(12),
                border: Border.all(color: AppTokens.border),
                boxShadow: const [
                  BoxShadow(
                    color: Color(0x66000000),
                    blurRadius: 12,
                    offset: Offset(0, 4),
                  ),
                ],
              ),
              clipBehavior: Clip.antiAlias,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _GitActionRow(
                    icon: 'git-commit',
                    label: 'Commit',
                    tooltip:
                        'Commit changes, staging all files first if needed',
                    enabled: hasChanges && !_running,
                    onTap: () => _run(projectId, _GitActionId.commit),
                  ),
                  _GitActionRow(
                    icon: 'cloud-upload',
                    label: 'Commit & Push',
                    tooltip: 'Commit changes and push, staging all files '
                        'first if needed',
                    enabled: hasChanges && !_running,
                    onTap: () => _run(projectId, _GitActionId.commitAndPush),
                  ),
                  const _MenuDivider(),
                  _GitActionRow(
                    icon: 'tool-download',
                    label: 'Fetch',
                    tooltip:
                        'Fetch remote updates without changing the local '
                        'checkout',
                    enabled: !_running,
                    onTap: () => _run(projectId, _GitActionId.fetch),
                  ),
                  _GitActionRow(
                    icon: 'git-pull',
                    label: _countLabel(
                      'Pull',
                      ref
                              .watch(activeGitStateProvider(projectId))
                              .valueOrNull
                              ?.behindCount ??
                          0,
                    ),
                    tooltip: 'Pull remote updates with fast-forward only',
                    enabled: !_running,
                    onTap: () => _run(projectId, _GitActionId.pull),
                  ),
                  _GitActionRow(
                    icon: 'cloud-upload',
                    label: _countLabel(
                      'Push',
                      ref
                              .watch(activeGitStateProvider(projectId))
                              .valueOrNull
                              ?.aheadCount ??
                          0,
                    ),
                    tooltip:
                        'Push the current checked-out branch to its remote',
                    enabled: !_running,
                    onTap: () => _run(projectId, _GitActionId.push),
                  ),
                  _GitActionRow(
                    icon: 'cloud-upload',
                    label: 'Force Push',
                    tooltip: 'Force-push with lease to overwrite the remote '
                        'branch if needed',
                    enabled: !_running,
                    danger: true,
                    onTap: () => _run(projectId, _GitActionId.forcePush),
                  ),
                  const _MenuDivider(),
                  _GitActionRow(
                    icon: 'github',
                    label: 'Create PR',
                    tooltip: 'Create a pull request for the current branch',
                    enabled: canCreatePr,
                    onTap: () => _run(projectId, _GitActionId.createPr),
                  ),
                  _GitActionRow(
                    icon: 'github',
                    label: 'Draft PR',
                    tooltip:
                        'Create a draft pull request for the current branch',
                    enabled: canCreatePr,
                    onTap: () => _run(projectId, _GitActionId.createDraftPr),
                  ),
                  // Create Branch is filed as a cross-module blocker
                  // — it opens a dedicated modal that hasn't been
                  // ported. The trailing divider + row land when
                  // that lands.
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }

  Future<void> _run(String projectId, _GitActionId action) async {
    setState(() {
      _menu.hide();
      _running = true;
    });
    final connection = ref.read(localConnectionProvider);
    final messenger = ScaffoldMessenger.maybeOf(context);
    try {
      final outcome = await connection.runToolbarGitAction(
        projectId: projectId,
        actionId: action.wireId,
      );
      if (mounted) {
        messenger?.showSnackBar(
          SnackBar(
            content: Text(outcome.toastMessage),
            backgroundColor: outcome.warning ? AppTokens.errorBg : null,
          ),
        );
      }
      if (outcome.refreshGitState) {
        ref.invalidate(changedFilesProvider(projectId));
        ref.invalidate(activeGitStateProvider(projectId));
        ref.invalidate(pullRequestStatusProvider(projectId));
      }
    } catch (e) {
      if (mounted) {
        messenger?.showSnackBar(
          SnackBar(
            content: Text(e.toString()),
            backgroundColor: AppTokens.errorBg,
          ),
        );
      }
    } finally {
      if (mounted) setState(() => _running = false);
    }
  }
}

/// Single dropdown row — h(34) px(12) gap(8). Disabled rows render
/// at 0.55 opacity, no hover bg, no click. Danger rows hover with
/// the danger tint instead of plain white@0.06.
class _GitActionRow extends StatefulWidget {
  const _GitActionRow({
    required this.icon,
    required this.label,
    required this.tooltip,
    required this.enabled,
    required this.onTap,
    this.danger = false,
  });

  final String icon;
  final String label;
  final String tooltip;
  final bool enabled;
  final VoidCallback onTap;
  final bool danger;

  @override
  State<_GitActionRow> createState() => _GitActionRowState();
}

class _GitActionRowState extends State<_GitActionRow> {
  bool _hover = false;

  // Danger col + danger hover from desktop/src/titlebar.rs:
  //   hsla(0, 0.78, 0.72) ≈ #EB7B7B text
  //   hsla(0, 0.45, 0.34, 0.26) ≈ rgba(126, 48, 48, 0.26) hover bg
  static const Color _dangerText = Color(0xFFEB7B7B);
  static const Color _dangerHover = Color(0x427E3030);

  @override
  Widget build(BuildContext context) {
    final hoverBg = widget.danger ? _dangerHover : AppTokens.overlayHover;
    final textColor =
        widget.danger ? _dangerText : const Color(0xEBFFFFFF); // 0.92
    final iconColor = textColor;
    final row = Opacity(
      opacity: widget.enabled ? 1.0 : 0.55,
      child: Container(
        height: 34,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        color: widget.enabled && _hover ? hoverBg : Colors.transparent,
        child: Row(
          children: [
            AppIcon(widget.icon, size: 14, color: iconColor),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                widget.label,
                overflow: TextOverflow.ellipsis,
                maxLines: 1,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w500,
                  color: textColor,
                ),
              ),
            ),
          ],
        ),
      ),
    );
    if (!widget.enabled) return row;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          child: row,
        ),
      ),
    );
  }
}

class _MenuDivider extends StatelessWidget {
  const _MenuDivider();

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 1,
      margin: const EdgeInsets.symmetric(horizontal: 8),
      color: AppTokens.border,
    );
  }
}
