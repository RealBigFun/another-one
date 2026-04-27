part of 'desktop_titlebar.dart';

/// Shared split-button shell used by the titlebar's action dropdowns.
/// It owns the overlay portal, hover state, and the two-half chrome so
/// each button only supplies its content and behavior.
class _TitlebarSplitButton extends StatefulWidget {
  const _TitlebarSplitButton({
    required this.buttonWidth,
    required this.menuWidth,
    required this.menuOpen,
    required this.onDismissMenu,
    required this.onPrimaryTap,
    required this.onChevronTap,
    required this.primaryBuilder,
    required this.chevronBuilder,
    required this.menuBuilder,
    this.primaryEnabled = true,
    this.chevronEnabled = true,
  });

  static const double buttonHeight = 28;
  static const double chevronWidth = 26;

  final double buttonWidth;
  final double menuWidth;
  final bool menuOpen;
  final bool primaryEnabled;
  final bool chevronEnabled;
  final VoidCallback onDismissMenu;
  final VoidCallback onPrimaryTap;
  final VoidCallback onChevronTap;
  final WidgetBuilder primaryBuilder;
  final WidgetBuilder chevronBuilder;
  final WidgetBuilder menuBuilder;

  @override
  State<_TitlebarSplitButton> createState() => _TitlebarSplitButtonState();
}

class _TitlebarSplitButtonState extends State<_TitlebarSplitButton> {
  final OverlayPortalController _menu = OverlayPortalController();
  final LayerLink _link = LayerLink();
  bool _primaryHover = false;
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
    _syncMenuVisibility(widget.menuOpen);
    final containerBg = widget.menuOpen
        ? AppTokens.overlayActive
        : AppTokens.overlayRest;
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: CompositedTransformTarget(
        link: _link,
        child: OverlayPortal(
          controller: _menu,
          overlayChildBuilder: _buildMenu,
          child: Container(
            width: widget.buttonWidth,
            height: _TitlebarSplitButton.buttonHeight,
            decoration: BoxDecoration(
              color: containerBg,
              borderRadius: BorderRadius.circular(11),
              border: Border.all(color: AppTokens.border),
            ),
            child: Row(
              children: [
                _buildPrimaryHalf(context),
                _buildChevronHalf(context),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildPrimaryHalf(BuildContext context) {
    final interactive = widget.primaryEnabled;
    return Expanded(
      child: MouseRegion(
        cursor: interactive
            ? SystemMouseCursors.click
            : SystemMouseCursors.basic,
        onEnter: interactive
            ? (_) => setState(() => _primaryHover = true)
            : null,
        onExit: interactive
            ? (_) => setState(() => _primaryHover = false)
            : null,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: interactive ? widget.onPrimaryTap : null,
          child: Container(
            alignment: Alignment.centerLeft,
            decoration: BoxDecoration(
              color: interactive && _primaryHover
                  ? AppTokens.overlayHoverStrong
                  : Colors.transparent,
              border: const Border(right: BorderSide(color: AppTokens.divider)),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 9),
            child: widget.primaryBuilder(context),
          ),
        ),
      ),
    );
  }

  Widget _buildChevronHalf(BuildContext context) {
    final interactive = widget.chevronEnabled;
    return MouseRegion(
      cursor: interactive ? SystemMouseCursors.click : SystemMouseCursors.basic,
      onEnter: interactive ? (_) => setState(() => _chevronHover = true) : null,
      onExit: interactive ? (_) => setState(() => _chevronHover = false) : null,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: interactive ? widget.onChevronTap : null,
        child: Container(
          width: _TitlebarSplitButton.chevronWidth,
          height: _TitlebarSplitButton.buttonHeight,
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
          child: widget.chevronBuilder(context),
        ),
      ),
    );
  }

  Widget _buildMenu(BuildContext context) {
    return Stack(
      children: [
        Positioned.fill(
          child: GestureDetector(
            behavior: HitTestBehavior.translucent,
            onTap: widget.onDismissMenu,
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
              width: widget.menuWidth,
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
              child: widget.menuBuilder(context),
            ),
          ),
        ),
      ],
    );
  }
}
