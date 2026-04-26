// Rotating refresh-icon spinner — Flutter port of
// `desktop/src/right_sidebar.rs::toolbar_spinner`. Same SVG
// (`icons__refresh.svg`), same 0.8s rotation period, same
// ease-in-out curve.
//
// Used wherever a per-action pending state replaces an in-place
// icon button: the right-sidebar Changes pane's stage / unstage /
// discard buttons swap to this widget while their git command is
// in flight.

import 'package:flutter/material.dart';

import 'app_icon.dart';

class ToolbarSpinner extends StatefulWidget {
  const ToolbarSpinner({
    super.key,
    required this.size,
    required this.color,
  });

  final double size;
  final Color color;

  @override
  State<ToolbarSpinner> createState() => _ToolbarSpinnerState();
}

class _ToolbarSpinnerState extends State<ToolbarSpinner>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller;
  late final Animation<double> _rotation;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 800),
    )..repeat();
    _rotation = CurvedAnimation(parent: _controller, curve: Curves.easeInOut);
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return RotationTransition(
      turns: _rotation,
      child: AppIcon('refresh', size: widget.size, color: widget.color),
    );
  }
}
