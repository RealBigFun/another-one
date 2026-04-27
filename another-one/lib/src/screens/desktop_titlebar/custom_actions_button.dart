// Split-button "Custom Actions" — primary half runs the most-
// recently-used action (or the first available, when none has
// been clicked yet), chevron half toggles a dropdown of every
// configured action with per-row Run / Edit affordances. Mirrors
// `desktop/src/titlebar.rs`'s
// `titlebar_custom_actions_button` + `titlebar_custom_actions_overlay`
// (GPUI splits them across two top-level chrome elements; here
// they fuse into one widget via `OverlayPortal`).
//
// Visibility gate: hidden when no project is active. With a
// project but no actions, the button still shows ("Actions" label
// + tool-bolt glyph) so the user can add their first action — same
// as GPUI.
//
// Modal launch: clicking the button when no actions exist, the
// per-row settings glyph, or the "Add action" footer row, all hit
// `openCustomActionModal` — the modal editor is a separate
// module (custom_action_modal.dart) and lives in 29i.3.

part of 'desktop_titlebar.dart';

class _CustomActionsButton extends ConsumerStatefulWidget {
  const _CustomActionsButton();

  @override
  ConsumerState<_CustomActionsButton> createState() =>
      _CustomActionsButtonState();
}

class _CustomActionsButtonState extends ConsumerState<_CustomActionsButton> {
  // Pulled from `desktop/src/titlebar.rs` constants so visual
  // metrics stay anchored to the GPUI source.
  static const double _buttonW = 148;
  static const double _buttonH = 28;
  static const double _chevronW = 26;
  static const double _menuW = 260;

  // GPUI's titlebar uses #2b2d31 as the dropdown surface (slightly
  // warmer than the chrome bg) — shared with the git-actions menu
  // body; lifted here as a constant so the two stay in sync.
  static const Color _menuBg = Color(0xFF2B2D31);

  final OverlayPortalController _menu = OverlayPortalController();
  final LayerLink _link = LayerLink();
  bool _bodyHover = false;
  bool _chevronHover = false;

  void _syncMenuVisibility(bool visible) {
    if (visible == _menu.isShowing) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || visible == _menu.isShowing) return;
      setState(visible ? _menu.show : _menu.hide);
    });
  }

  @override
  Widget build(BuildContext context) {
    final projectId = ref.watch(activeProjectIdProvider);
    if (projectId == null) return const SizedBox.shrink();
    final actions =
        ref.watch(projectActionsProvider(projectId)).valueOrNull ??
        const <ProjectActionDto>[];
    final selected = _selectedAction(ref, actions);
    final menuOpen =
        ref.watch(_activeTitlebarDropdownProvider) ==
        _TitlebarDropdown.customActions;
    _syncMenuVisibility(menuOpen);
    final containerBg = menuOpen
        ? AppTokens.overlayActive
        : AppTokens.overlayRest;
    final iconPath = selected != null
        ? _actionIconPath(selected.icon)
        : 'assets/icons/icons__tool-bolt.svg';
    final label = selected?.name.trim().isNotEmpty == true
        ? selected!.name
        : (selected != null ? _kindLabel(selected.kind) : 'Actions');

    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: CompositedTransformTarget(
        link: _link,
        child: OverlayPortal(
          controller: _menu,
          overlayChildBuilder: (context) =>
              _buildMenu(context, projectId, actions),
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
                _buildPrimaryHalf(projectId, selected, iconPath, label),
                _buildChevronHalf(projectId, actions),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildPrimaryHalf(
    String projectId,
    ProjectActionDto? selected,
    String iconPath,
    String label,
  ) {
    return Expanded(
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _bodyHover = true),
        onExit: (_) => setState(() => _bodyHover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () {
            _dismissTitlebarDropdowns(ref);
            if (selected != null) {
              unawaited(_runAction(projectId, selected));
            } else {
              _openModal(null);
            }
          },
          child: Container(
            decoration: BoxDecoration(
              color: _bodyHover
                  ? AppTokens.overlayHoverStrong
                  : Colors.transparent,
              border: const Border(right: BorderSide(color: AppTokens.divider)),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 9),
            child: Row(
              children: [
                SvgPicture.asset(
                  iconPath,
                  width: 14,
                  height: 14,
                  colorFilter: const ColorFilter.mode(
                    Color(0xEBFFFFFF),
                    BlendMode.srcIn,
                  ),
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    label,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w500,
                      color: Color(0xDBFFFFFF),
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

  Widget _buildChevronHalf(String projectId, List<ProjectActionDto> actions) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _chevronHover = true),
      onExit: (_) => setState(() => _chevronHover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () {
          if (actions.isEmpty) {
            // Mirrors GPUI's "if no actions exist, jump straight to
            // the modal instead of opening an empty dropdown".
            _dismissTitlebarDropdowns(ref);
            _openModal(null);
            return;
          }
          _toggleTitlebarDropdown(ref, _TitlebarDropdown.customActions);
        },
        child: Container(
          width: _chevronW,
          height: _buttonH,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: _chevronHover
                ? AppTokens.overlayHoverStrong
                : Colors.transparent,
            borderRadius: const BorderRadius.only(
              topRight: Radius.circular(11),
              bottomRight: Radius.circular(11),
            ),
          ),
          child: SvgPicture.asset(
            'assets/icons/icons__chevron-down.svg',
            width: 11,
            height: 11,
            colorFilter: const ColorFilter.mode(
              Color(0xADFFFFFF),
              BlendMode.srcIn,
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildMenu(
    BuildContext context,
    String projectId,
    List<ProjectActionDto> actions,
  ) {
    return Stack(
      children: [
        Positioned.fill(
          child: GestureDetector(
            behavior: HitTestBehavior.translucent,
            onTap: () => _dismissTitlebarDropdowns(ref),
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
                color: _menuBg,
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
                children: [
                  for (final action in actions)
                    _CustomActionMenuRow(
                      action: action,
                      onRun: () {
                        _dismissTitlebarDropdowns(ref);
                        unawaited(_runAction(projectId, action));
                      },
                      onEdit: () {
                        _dismissTitlebarDropdowns(ref);
                        _openModal(action);
                      },
                    ),
                  Container(
                    height: 1,
                    margin: const EdgeInsets.symmetric(horizontal: 8),
                    color: const Color(0x14FFFFFF),
                  ),
                  _CustomActionAddRow(
                    onTap: () {
                      _dismissTitlebarDropdowns(ref);
                      _openModal(null);
                    },
                  ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }

  /// Pick the action the primary half should run on click.
  /// Mirrors GPUI's `selected_custom_action`: most-recently-clicked
  /// id when still present, else the first action in the list.
  ProjectActionDto? _selectedAction(
    WidgetRef ref,
    List<ProjectActionDto> actions,
  ) {
    if (actions.isEmpty) return null;
    final lastUsed = ref.watch(lastUsedCustomActionIdProvider);
    if (lastUsed != null) {
      for (final a in actions) {
        if (a.id == lastUsed) return a;
      }
    }
    return actions.first;
  }

  Future<void> _runAction(String projectId, ProjectActionDto action) async {
    final selection = ref.read(selectedTabProvider);
    if (selection == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('Custom actions run inside an active task.'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
      return;
    }
    final connection = ref.read(localConnectionProvider);
    try {
      await connection.runProjectAction(
        projectId: projectId,
        sectionId: selection.sectionId,
        actionId: action.id,
      );
      ref.read(lastUsedCustomActionIdProvider.notifier).state = action.id;
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Action failed: $e'),
          backgroundColor: AppTokens.errorBg,
        ),
      );
    }
  }

  Future<void> _openModal(ProjectActionDto? edit) async {
    final projectId = ref.read(activeProjectIdProvider);
    if (projectId == null) return;
    final saved = await showCustomActionModal(
      context: context,
      projectId: projectId,
      existing: edit,
    );
    if (!mounted || !saved) return;
    ref.invalidate(projectActionsProvider(projectId));
  }
}

/// A single dropdown row for one custom action. Left side runs on
/// tap; right side has a globe glyph (when scope is global) and a
/// settings cog that opens the editor for this action.
class _CustomActionMenuRow extends StatefulWidget {
  const _CustomActionMenuRow({
    required this.action,
    required this.onRun,
    required this.onEdit,
  });

  final ProjectActionDto action;
  final VoidCallback onRun;
  final VoidCallback onEdit;

  @override
  State<_CustomActionMenuRow> createState() => _CustomActionMenuRowState();
}

class _CustomActionMenuRowState extends State<_CustomActionMenuRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final action = widget.action;
    final isGlobal = action.scope == ProjectActionScopeDto.global;
    final label = action.name.trim().isNotEmpty
        ? action.name
        : _kindLabel(action.kind);
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Container(
        height: 36,
        color: _hover ? const Color(0x0FFFFFFF) : Colors.transparent,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        child: Row(
          children: [
            Expanded(
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: widget.onRun,
                child: MouseRegion(
                  cursor: SystemMouseCursors.click,
                  child: Row(
                    children: [
                      SvgPicture.asset(
                        _actionIconPath(action.icon),
                        width: 14,
                        height: 14,
                        colorFilter: const ColorFilter.mode(
                          Color(0xEBFFFFFF),
                          BlendMode.srcIn,
                        ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          label,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w500,
                            color: Color(0xEBFFFFFF),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            if (isGlobal)
              Padding(
                padding: const EdgeInsets.only(right: 0),
                child: SizedBox(
                  width: 20,
                  height: 24,
                  child: Center(
                    child: SvgPicture.asset(
                      'assets/icons/icons__globe.svg',
                      width: 13,
                      height: 13,
                      colorFilter: const ColorFilter.mode(
                        Color(0x8AFFFFFF),
                        BlendMode.srcIn,
                      ),
                    ),
                  ),
                ),
              ),
            _SettingsGlyphButton(onTap: widget.onEdit),
          ],
        ),
      ),
    );
  }
}

/// Cog button on each row. Hover bg lifts to white@0.08; idle is
/// transparent so the row's own hover bg shows through.
class _SettingsGlyphButton extends StatefulWidget {
  const _SettingsGlyphButton({required this.onTap});
  final VoidCallback onTap;

  @override
  State<_SettingsGlyphButton> createState() => _SettingsGlyphButtonState();
}

class _SettingsGlyphButtonState extends State<_SettingsGlyphButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          width: 24,
          height: 24,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: _hover ? const Color(0x14FFFFFF) : Colors.transparent,
            borderRadius: BorderRadius.circular(6),
          ),
          child: SvgPicture.asset(
            'assets/icons/icons__settings.svg',
            width: 13,
            height: 13,
            colorFilter: const ColorFilter.mode(
              Color(0x8AFFFFFF),
              BlendMode.srcIn,
            ),
          ),
        ),
      ),
    );
  }
}

class _CustomActionAddRow extends StatefulWidget {
  const _CustomActionAddRow({required this.onTap});
  final VoidCallback onTap;

  @override
  State<_CustomActionAddRow> createState() => _CustomActionAddRowState();
}

class _CustomActionAddRowState extends State<_CustomActionAddRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: 36,
          color: _hover ? const Color(0x0FFFFFFF) : Colors.transparent,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: Row(
            children: [
              SvgPicture.asset(
                'assets/icons/icons__plus.svg',
                width: 14,
                height: 14,
                colorFilter: const ColorFilter.mode(
                  Color(0xEBFFFFFF),
                  BlendMode.srcIn,
                ),
              ),
              const SizedBox(width: 8),
              const Text(
                'Add action',
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w500,
                  color: Color(0xEBFFFFFF),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

String _actionIconPath(ProjectActionIconDto icon) {
  switch (icon) {
    case ProjectActionIconDto.play:
      return 'assets/icons/action__play.svg';
    case ProjectActionIconDto.test:
      return 'assets/icons/action__test.svg';
    case ProjectActionIconDto.lint:
      return 'assets/icons/action__lint.svg';
    case ProjectActionIconDto.configure:
      return 'assets/icons/action__configure.svg';
    case ProjectActionIconDto.build:
      return 'assets/icons/action__build.svg';
    case ProjectActionIconDto.debug:
      return 'assets/icons/action__debug.svg';
    case ProjectActionIconDto.agent:
      return 'assets/icons/action__agent.svg';
  }
}

/// Display fallback for an action with a blank `name`. Mirrors
/// `ProjectAction::display_name()` in `core::project_store`.
String _kindLabel(ProjectActionKindDto kind) {
  return switch (kind) {
    ProjectActionKindDto_Shell() => 'Shell action',
    ProjectActionKindDto_Agent() => 'Agent action',
  };
}
