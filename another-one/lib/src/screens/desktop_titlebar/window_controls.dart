part of 'desktop_titlebar.dart';

enum _WindowControlKind { minimize, maximize, close }

class _LinuxWindowControls extends ConsumerWidget {
  const _LinuxWindowControls();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Padding(
      padding: const EdgeInsets.only(left: AppTokens.space2),
      child: Row(
        children: [
          _WindowControlButton(
            kind: _WindowControlKind.minimize,
            tooltip: 'Minimize window',
            onPressed: () {
              _dismissTitlebarDropdowns(ref);
              unawaited(minimizeNativeWindow());
            },
          ),
          const SizedBox(width: 2),
          _WindowControlButton(
            kind: _WindowControlKind.maximize,
            tooltip: 'Maximize or restore window',
            onPressed: () {
              _dismissTitlebarDropdowns(ref);
              unawaited(toggleNativeWindowMaximize());
            },
          ),
          const SizedBox(width: 2),
          _WindowControlButton(
            kind: _WindowControlKind.close,
            tooltip: 'Close window',
            onPressed: () {
              _dismissTitlebarDropdowns(ref);
              unawaited(closeNativeWindow());
            },
          ),
        ],
      ),
    );
  }
}

class _WindowControlButton extends StatefulWidget {
  const _WindowControlButton({
    required this.kind,
    required this.tooltip,
    required this.onPressed,
  });

  final _WindowControlKind kind;
  final String tooltip;
  final VoidCallback onPressed;

  @override
  State<_WindowControlButton> createState() => _WindowControlButtonState();
}

class _WindowControlButtonState extends State<_WindowControlButton> {
  bool _hover = false;

  bool get _danger => widget.kind == _WindowControlKind.close;

  @override
  Widget build(BuildContext context) {
    final background = _hover
        ? (_danger ? AppTokens.errorBg : AppTokens.overlayHoverStrong)
        : Colors.transparent;
    final foreground = _hover && _danger
        ? AppTokens.textPrimary
        : AppTokens.textSecondary;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: Container(
            width: 28,
            height: 28,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: background,
              borderRadius: BorderRadius.circular(AppTokens.radiusMd),
            ),
            child: _WindowControlGlyph(kind: widget.kind, color: foreground),
          ),
        ),
      ),
    );
  }
}

class _WindowControlGlyph extends StatelessWidget {
  const _WindowControlGlyph({required this.kind, required this.color});

  final _WindowControlKind kind;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return switch (kind) {
      _WindowControlKind.minimize => SvgPicture.asset(
        'assets/icons/icons__minus.svg',
        width: 10,
        height: 10,
        colorFilter: ColorFilter.mode(color, BlendMode.srcIn),
      ),
      _WindowControlKind.maximize => Container(
        width: 10,
        height: 9,
        decoration: BoxDecoration(
          border: Border.all(color: color, width: 1),
          borderRadius: BorderRadius.circular(1.5),
        ),
      ),
      _WindowControlKind.close => SvgPicture.asset(
        'assets/icons/icons__close.svg',
        width: 10,
        height: 10,
        colorFilter: ColorFilter.mode(color, BlendMode.srcIn),
      ),
    };
  }
}
