// Open PRs section on the project overview page — pixel-precise
// port of `desktop/src/project_page.rs::project_page_prs_section`
// + project_page_pr_row.
//
// Top to bottom:
//   1. Collapsible header: chevron + 'Open PRs' + count pill
//   2. Filter tabs (All Open / Needs My Review / My PRs / Draft)
//   3. Search bar: input + Apply + Clear
//   4. Syntax hint
//   5. Loading / empty / error / list of PR rows
//
// The Review button on each row spawns a worktree task targeting
// the PR's head branch and selects it as the active section; the
// shell switches to the new task immediately to match GPUI's
// behavior.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../rust/api/local_session.dart' show ProjectPagePullRequestDto;
import '../../state/local_connection_provider.dart';
import '../../state/project_pull_requests_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../widgets/app_icon.dart';
import '../../widgets/run_mutation.dart';

const List<String> _kFilterLabels = [
  'All Open',
  'Needs My Review',
  'My PRs',
  'Draft',
];

class OpenPrsSection extends ConsumerWidget {
  const OpenPrsSection({super.key, required this.projectId});

  final String projectId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final expanded = ref.watch(openPrsExpandedProvider);
    final search = ref.watch(projectPrSearchProvider(projectId));
    final prsAsync = ref.watch(projectPullRequestsProvider(
      ProjectPullRequestsKey(
        projectId: projectId,
        filterIndex: search.filterIndex,
        query: search.appliedQuery,
      ),
    ));
    final count = prsAsync.valueOrNull?.length ?? 0;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _OpenPrsHeader(expanded: expanded, count: count),
        if (expanded) ...[
          const SizedBox(height: 12),
          _FilterTabs(
            projectId: projectId,
            activeIndex: search.filterIndex,
          ),
          const SizedBox(height: 12),
          _SearchBar(projectId: projectId),
          const SizedBox(height: 12),
          const Text(
            'Use GitHub PR search syntax like review-requested:@me, '
            'author:@me, draft:true, or free-text terms.',
            style: TextStyle(
              fontSize: 10,
              color: AppTokens.textMuted,
            ),
          ),
          const SizedBox(height: 12),
          _PrList(projectId: projectId, prs: prsAsync),
        ],
      ],
    );
  }
}

class _OpenPrsHeader extends ConsumerStatefulWidget {
  const _OpenPrsHeader({required this.expanded, required this.count});

  final bool expanded;
  final int count;

  @override
  ConsumerState<_OpenPrsHeader> createState() => _OpenPrsHeaderState();
}

class _OpenPrsHeaderState extends ConsumerState<_OpenPrsHeader> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () =>
            ref.read(openPrsExpandedProvider.notifier).update((s) => !s),
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: 4),
          color: _hover ? const Color(0x08FFFFFF) : Colors.transparent,
          child: Row(
            children: [
              AppIcon(
                widget.expanded ? 'chevron-down' : 'chevron-right',
                size: 16,
                color: AppTokens.textSecondary,
              ),
              const SizedBox(width: 8),
              const Text(
                'Open PRs',
                style: TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w600,
                  color: AppTokens.textPrimary,
                ),
              ),
              const SizedBox(width: 8),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                decoration: BoxDecoration(
                  color: const Color(0x1AFFFFFF), // white @ 0.10
                  borderRadius: BorderRadius.circular(10),
                ),
                child: Text(
                  '${widget.count}',
                  style: const TextStyle(
                    fontSize: 10,
                    color: AppTokens.textSecondary,
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _FilterTabs extends ConsumerWidget {
  const _FilterTabs({required this.projectId, required this.activeIndex});

  final String projectId;
  final int activeIndex;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Wrap(
        spacing: 4,
        children: [
          for (var i = 0; i < _kFilterLabels.length; i++)
            _FilterTab(
              label: _kFilterLabels[i],
              active: i == activeIndex,
              onTap: () => ref
                  .read(projectPrSearchProvider(projectId).notifier)
                  .setFilter(i),
            ),
        ],
      ),
    );
  }
}

class _FilterTab extends StatefulWidget {
  const _FilterTab({
    required this.label,
    required this.active,
    required this.onTap,
  });

  final String label;
  final bool active;
  final VoidCallback onTap;

  @override
  State<_FilterTab> createState() => _FilterTabState();
}

class _FilterTabState extends State<_FilterTab> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final bg = widget.active
        ? const Color(0x1AFFFFFF) // white @ 0.10
        : (_hover ? const Color(0x0DFFFFFF) /* white @ 0.05 */ : Colors.transparent);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: 26,
          padding: const EdgeInsets.symmetric(horizontal: 7),
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(5),
          ),
          child: Text(
            widget.label,
            style: TextStyle(
              fontSize: 11,
              fontWeight: widget.active ? FontWeight.w500 : FontWeight.w400,
              color: widget.active
                  ? AppTokens.textPrimary
                  : AppTokens.textSecondary,
            ),
          ),
        ),
      ),
    );
  }
}

class _SearchBar extends ConsumerStatefulWidget {
  const _SearchBar({required this.projectId});

  final String projectId;

  @override
  ConsumerState<_SearchBar> createState() => _SearchBarState();
}

class _SearchBarState extends ConsumerState<_SearchBar> {
  late final TextEditingController _controller;
  late final FocusNode _focusNode;

  @override
  void initState() {
    super.initState();
    final initial =
        ref.read(projectPrSearchProvider(widget.projectId)).draftQuery;
    _controller = TextEditingController(text: initial);
    _focusNode = FocusNode();
  }

  @override
  void dispose() {
    _controller.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  void _apply() {
    final notifier = ref.read(projectPrSearchProvider(widget.projectId).notifier);
    notifier.setDraft(_controller.text);
    notifier.apply();
  }

  void _clear() {
    _controller.clear();
    ref.read(projectPrSearchProvider(widget.projectId).notifier).clear();
  }

  @override
  Widget build(BuildContext context) {
    final search = ref.watch(projectPrSearchProvider(widget.projectId));
    if (_controller.text != search.draftQuery) {
      _controller.text = search.draftQuery;
    }
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        Expanded(
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            decoration: BoxDecoration(
              color: const Color(0x0DFFFFFF), // white @ 0.05
              borderRadius: BorderRadius.circular(7),
              border: Border.all(color: AppTokens.border),
            ),
            child: Row(
              children: [
                const AppIcon(
                  'file_icons__magnifying_glass',
                  size: 14,
                  color: AppTokens.textMuted,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: TextField(
                    controller: _controller,
                    focusNode: _focusNode,
                    onChanged: (value) => ref
                        .read(projectPrSearchProvider(widget.projectId).notifier)
                        .setDraft(value),
                    onSubmitted: (_) => _apply(),
                    style: const TextStyle(
                      fontSize: 12,
                      color: AppTokens.textPrimary,
                    ),
                    decoration: const InputDecoration(
                      isDense: true,
                      contentPadding: EdgeInsets.zero,
                      border: InputBorder.none,
                      hintText:
                          'GitHub query, e.g. author:@me review-requested:@me',
                      hintStyle: TextStyle(
                        fontSize: 12,
                        color: AppTokens.textMuted,
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(width: 8),
        _PillButton(
          label: 'Apply',
          tooltip: 'Run the search query against open PRs',
          fontWeight: FontWeight.w600,
          color: AppTokens.textPrimary,
          background: const Color(0xFF1E2024),
          showBorder: true,
          onTap: _apply,
        ),
        const SizedBox(width: 8),
        _PillButton(
          label: 'Clear',
          tooltip: 'Clear the search query',
          fontWeight: FontWeight.w600,
          color: AppTokens.textSecondary,
          background: Colors.transparent,
          showBorder: false,
          onTap: _clear,
        ),
      ],
    );
  }
}

/// Apply / Clear pill matching GPUI's two buttons in the search
/// bar. Apply: bordered, dark fill; Clear: borderless, hover-only.
class _PillButton extends StatefulWidget {
  const _PillButton({
    required this.label,
    required this.tooltip,
    required this.fontWeight,
    required this.color,
    required this.background,
    required this.showBorder,
    required this.onTap,
  });

  final String label;
  final String tooltip;
  final FontWeight fontWeight;
  final Color color;
  final Color background;
  final bool showBorder;
  final VoidCallback onTap;

  @override
  State<_PillButton> createState() => _PillButtonState();
}

class _PillButtonState extends State<_PillButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final bg = _hover ? const Color(0x0FFFFFFF) /* white @ 0.06 */ : widget.background;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          child: Container(
            height: 30,
            padding: const EdgeInsets.symmetric(horizontal: 7),
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: bg,
              borderRadius: BorderRadius.circular(7),
              border: widget.showBorder
                  ? Border.all(color: AppTokens.border)
                  : null,
            ),
            child: Text(
              widget.label,
              style: TextStyle(
                fontSize: 11,
                fontWeight: widget.fontWeight,
                color: widget.color,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _PrList extends ConsumerWidget {
  const _PrList({required this.projectId, required this.prs});

  final String projectId;
  final AsyncValue<List<ProjectPagePullRequestDto>?> prs;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return prs.when(
      data: (list) {
        if (list == null) {
          return const Text(
            'Loading pull requests...',
            style: TextStyle(fontSize: 12, color: AppTokens.textMuted),
          );
        }
        if (list.isEmpty) {
          return const Text(
            'No matching open pull requests.',
            style: TextStyle(fontSize: 12, color: AppTokens.textMuted),
          );
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (final pr in list) ...[
              _PullRequestRow(projectId: projectId, pr: pr),
              const SizedBox(height: 8),
            ],
          ],
        );
      },
      loading: () => const Text(
        'Loading pull requests...',
        style: TextStyle(fontSize: 12, color: AppTokens.textMuted),
      ),
      error: (e, _) => Text(
        e.toString(),
        style: const TextStyle(fontSize: 12, color: AppTokens.textMuted),
      ),
    );
  }
}

class _PullRequestRow extends ConsumerStatefulWidget {
  const _PullRequestRow({required this.projectId, required this.pr});

  final String projectId;
  final ProjectPagePullRequestDto pr;

  @override
  ConsumerState<_PullRequestRow> createState() => _PullRequestRowState();
}

class _PullRequestRowState extends ConsumerState<_PullRequestRow> {
  bool _hover = false;

  static const Color _green = Color(0xFFA0D9B4); // hsla(138/360, 0.50, 0.74)
  static const Color _red = Color(0xFFEBA8B0); // hsla(352/360, 0.52, 0.76)

  @override
  Widget build(BuildContext context) {
    final pr = widget.pr;
    final ciIcon = pr.reviewRequired ? 'badge-x' : 'badge-check';
    final ciColor = pr.reviewRequired ? _red : _green;
    final showBadge = pr.reviewRequired || pr.draft;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Container(
        padding: const EdgeInsets.all(12),
        decoration: BoxDecoration(
          color: _hover
              ? const Color(0x0FFFFFFF) // white @ 0.06
              : const Color(0x08FFFFFF), // white @ 0.03
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: AppTokens.divider),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // Top line
            Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                _PrNumberBadge(number: pr.number, url: pr.url),
                const SizedBox(width: 8),
                AppIcon(ciIcon, size: 16, color: ciColor),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    pr.title,
                    overflow: TextOverflow.ellipsis,
                    maxLines: 1,
                    style: const TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w500,
                      color: AppTokens.textPrimary,
                    ),
                  ),
                ),
                if (showBadge) ...[
                  const SizedBox(width: 8),
                  _ReviewStatusChip(label: pr.draft ? 'Draft' : 'Review required'),
                ],
              ],
            ),
            const SizedBox(height: 6),
            // Bottom line
            Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 200),
                  child: Text(
                    pr.branch,
                    overflow: TextOverflow.ellipsis,
                    maxLines: 1,
                    style: const TextStyle(
                      fontSize: 10,
                      color: AppTokens.textMuted,
                      fontFamily: AppTokens.fontFamilyMono,
                    ),
                  ),
                ),
                const SizedBox(width: 6),
                const Text(
                  '·',
                  style: TextStyle(
                    fontSize: 10,
                    color: AppTokens.textMuted,
                  ),
                ),
                const SizedBox(width: 6),
                Flexible(
                  child: Text(
                    pr.author,
                    overflow: TextOverflow.ellipsis,
                    maxLines: 1,
                    style: const TextStyle(
                      fontSize: 10,
                      color: AppTokens.textSecondary,
                    ),
                  ),
                ),
                const SizedBox(width: 6),
                const Text(
                  '·',
                  style: TextStyle(
                    fontSize: 10,
                    color: AppTokens.textMuted,
                  ),
                ),
                const SizedBox(width: 6),
                Text(
                  '+${pr.linesAdded}',
                  style: const TextStyle(
                    fontSize: 10,
                    fontWeight: FontWeight.w600,
                    color: _green,
                  ),
                ),
                const SizedBox(width: 4),
                Text(
                  '-${pr.linesRemoved}',
                  style: const TextStyle(
                    fontSize: 10,
                    fontWeight: FontWeight.w600,
                    color: _red,
                  ),
                ),
                const Spacer(),
                _ReviewButton(
                  projectId: widget.projectId,
                  pullRequestNumber: pr.number,
                  headBranch: pr.branch,
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _PrNumberBadge extends StatefulWidget {
  const _PrNumberBadge({required this.number, required this.url});

  final BigInt number;
  final String url;

  @override
  State<_PrNumberBadge> createState() => _PrNumberBadgeState();
}

class _PrNumberBadgeState extends State<_PrNumberBadge> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: 'Open pull request in GitHub',
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () async {
            final uri = Uri.tryParse(widget.url);
            if (uri == null) return;
            await launchUrl(uri, mode: LaunchMode.externalApplication);
          },
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
            decoration: BoxDecoration(
              color: _hover
                  ? const Color(0x24FFFFFF) // white @ 0.14
                  : const Color(0x14FFFFFF), // white @ 0.08
              borderRadius: BorderRadius.circular(5),
            ),
            child: Text(
              '#${widget.number}',
              style: TextStyle(
                fontSize: 10,
                color: _hover
                    ? AppTokens.textPrimary
                    : AppTokens.textSecondary,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Pill that says 'Draft' or 'Review required'. Amber palette.
class _ReviewStatusChip extends StatelessWidget {
  const _ReviewStatusChip({required this.label});

  final String label;

  // hsla(30/360, 0.70, 0.35) ≈ #944A0F bg
  static const Color _bg = Color(0xFF944A0F);
  // hsla(30/360, 0.90, 0.80) ≈ #F8C997 text
  static const Color _text = Color(0xFFF8C997);
  // hsla(30/360, 0.90, 0.65) ≈ #F2A052 dot
  static const Color _dot = Color(0xFFF2A052);

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(
        color: _bg,
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 6,
            height: 6,
            decoration: const BoxDecoration(
              color: _dot,
              shape: BoxShape.circle,
            ),
          ),
          const SizedBox(width: 4),
          Text(
            label,
            style: const TextStyle(
              fontSize: 10,
              fontWeight: FontWeight.w500,
              color: _text,
            ),
          ),
        ],
      ),
    );
  }
}

class _ReviewButton extends ConsumerStatefulWidget {
  const _ReviewButton({
    required this.projectId,
    required this.pullRequestNumber,
    required this.headBranch,
  });

  final String projectId;
  final BigInt pullRequestNumber;
  final String headBranch;

  @override
  ConsumerState<_ReviewButton> createState() => _ReviewButtonState();
}

class _ReviewButtonState extends ConsumerState<_ReviewButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: 'Open a review task for this pull request',
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => _spawn(context, ref),
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            decoration: BoxDecoration(
              color: _hover
                  ? const Color(0x2EFFFFFF) // white @ 0.18
                  : const Color(0x1AFFFFFF), // white @ 0.10
              borderRadius: BorderRadius.circular(6),
            ),
            child: const Text(
              'Review',
              style: TextStyle(
                fontSize: 10,
                fontWeight: FontWeight.w500,
                color: AppTokens.textPrimary,
              ),
            ),
          ),
        ),
      ),
    );
  }

  Future<void> _spawn(BuildContext context, WidgetRef ref) async {
    final connection = ref.read(localConnectionProvider);
    final result = await runMutation<String>(
      context,
      () => connection.createReviewTask(
        projectId: widget.projectId,
        pullRequestNumber: widget.pullRequestNumber.toInt(),
        headBranch: widget.headBranch,
        // Pass null so the daemon picks the user's default agent
        // (matches GPUI's terminal_launch_config_for_selected_agent
        // path).
      ),
      errorPrefix: 'Could not create review task',
    );
    if (result == null) return;
    // Switch the centre pane to the new task — same end-state as
    // GPUI's launch_task_request flow.
    ref.read(selectedTabProvider.notifier).set(
          TabSelection(sectionId: result, tabId: '0'),
        );
  }
}
