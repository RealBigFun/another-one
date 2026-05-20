# User-Facing Feature Inventory

This document lists the features that a user of `another-one` can see, trigger, or interact with directly in the app UI.

## 1. Project Management

### 1.1 Project list and grouping
- View projects in the left sidebar.
- View related repo roots and worktrees grouped together.
- Expand and collapse grouped project/task sections.
- Select a project to view its project page.

### 1.2 Project actions
- Remove a project group from the app.
- Open a project's GitHub repository link.
- View a project detail page in the main workspace area.

## 2. Task Management

### 2.1 Create tasks
- Create a new task from a project.
- Choose the source branch when creating a task.
- Choose whether the task is created as a worktree or directly in the original project.
- Enter or edit the task name during creation.
- Create a task with one or more selected agents.

### 2.2 Manage tasks
- Rename a task.
- Delete a task.
- Pin a task.
- Unpin a task.
- Switch between tasks.
- Jump to next or previous task with shortcuts.

## 3. Agent and Terminal Workflows

### 3.1 Add and manage agent tabs
- Add a new agent tab to a task.
- Open multiple tabs within a task.
- Close an existing tab.
- Switch between tabs.
- Reopen or restore terminal tabs associated with a task.

### 3.2 Choose agent providers
- Launch a task using Claude Code.
- Launch a task using Codex.
- Launch a task using Cursor Agent.
- Launch a task using Gemini.
- Launch a task using Pi.
- Launch a task using OpenCode.
- Launch a task using Amp.
- Launch a task using Rovo Dev.
- Launch a task using Forge.
- Launch a raw shell terminal instead of an agent.

### 3.3 Embedded terminal interaction
- View terminal output directly inside the app.
- Resize terminal content with the UI layout.
- Copy terminal output.
- Paste into terminal sessions.
- Preview pasted images.
- View tab titles that reflect the current terminal or agent session.
- Search terminal scrollback output.

## 4. Git and Branch Workflow

### 4.1 Changed files sidebar
- View changed files in the right sidebar.
- View files separated into staged and unstaged groups.
- View file status indicators.
- View file-level addition and deletion counts.

### 4.2 File actions
- Stage an individual file.
- Unstage an individual file.
- Stage a section of files.
- Unstage a section of files.

### 4.3 Branch and repo status
- View branch ahead/behind state.
- View branch compare state.
- View recent commit history.
- View commit-level file changes.

### 4.4 Git actions
- Commit changes.
- Commit and push changes.
- Fetch remote changes.
- Pull remote changes.
- Push local changes.
- Force push changes.
- Undo the last commit.
- Create a pull request.
- Create a draft pull request.

## 5. Pull Request Visibility

### 5.1 Pull request awareness
- View pull request status for the active branch.
- View CI/check run status for the active branch.
- View pull request cards on the project page.

### 5.2 Pull request card details
- View pull request number.
- View pull request title.
- View author.
- View associated branch name.
- View lines added and removed.
- View whether CI has passed.
- View whether review is required.
- View reviewer chips.

### 5.3 Pull request filtering UI
- Filter visible pull requests by category.
- View tabs such as All Open, Needs My Review, My PRs, and Draft.

## 6. Settings and Personalization

### 6.1 Settings sections
- Open the Settings page.
- View separate settings areas for Agents, Open In, and Keybindings.

### 6.2 Agent settings
- Configure agent-related launch behavior.
- Adjust per-agent launch arguments.

### 6.3 Open In settings
- Configure which external apps are available for opening projects.

### 6.4 Keyboard shortcuts
- View configured keyboard shortcuts.
- Change a shortcut.
- Reset an individual shortcut.
- Reset all shortcuts.

### 6.5 Theme
- Toggle between dark mode and light mode.
- Automatic OS appearance detection.

### 6.6 MCP server management
- Add MCP servers to the agent environment.
- Toggle individual MCP servers on or off per agent.
- Remove MCP servers from the configuration.

## 7. Open In Integrations

### 7.1 External app launching
- Open a project in Cursor.
- Open a project in Zed.
- Open a project in VS Code.
- Open a project in the system file manager.

## 8. Resource Usage Visibility

### 8.1 Resource indicator
- View app CPU and memory usage from the titlebar resource indicator.
- Open a detailed resource usage overlay.
- Refresh resource usage manually.

### 8.2 Resource usage tree
- View resource usage grouped by project.
- View resource usage grouped by task.
- View resource usage per terminal session.
- Expand and collapse resource usage sections.

## 9. Window and Interface Controls

### 9.1 Layout controls
- Toggle the left sidebar.
- Toggle the right sidebar.
- Use a custom titlebar on supported platforms.

### 9.2 Zoom controls
- Zoom in.
- Zoom out.
- Reset zoom.

### 9.3 UI feedback
- See tooltips on interactive controls.
- See toast notifications and temporary feedback messages.

### 9.4 App updates
- Automatic background update checks.
- In-app install prompt when an update is available.

## 10. Platform Notes

This inventory covers the desktop app (macOS and Linux). The app also targets Android and iOS via a native GPUI mobile shell; mobile-specific UI surfaces are not covered here.
