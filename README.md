# tmux-gui — Claude Code projects

A desktop window that lists your projects and starts or manages a Claude Code session in each one, using tmux underneath. No tmux keyboard shortcuts needed.

## Layout
By default the window is a narrow project list. A tab on its right edge opens the preview panel and widens the window; clicking the tab again hides the panel and shrinks the window back.

- **Left**: your registered projects. A project is a folder. If a tmux session was started in that folder, the project shows it: a green dot means a terminal window is open on it, a yellow dot means it runs with no terminal open, and a dimmed name with a faint ring means nothing is running. Windows of a running session are nested under the project. tmux sessions that belong to no project are listed under "Other tmux sessions".
- **Right**: the selected project, one primary button, an "Actions" menu, a live view of the terminal text refreshed every second, and a command box underneath.

## How to do things
- "+ Add project" opens a folder picker. The project name defaults to the folder name.
- Double-click a project that is not running: a tmux session is created in its folder, `claude --continue` is typed into it, and a terminal opens on it.
- Double-click a running project (or window): a terminal opens on it.
- Right-click any row for its actions. The same actions are in the "Actions" menu on the right.
- Project actions: start Claude Code (or, when running: open in terminal, new window, close terminals, kill session), open folder in the file manager, rename project, change folder, remove from list. Renaming a project also renames its running session.
- Window actions: open in terminal here, make active, rename, kill.
- Command box: type a command line and press Enter. It is typed into the selected window followed by Enter. For anything interactive, open the terminal.
- Killing anything asks for confirmation first. Removing a project only edits the list; the folder and any session are untouched.

The project list is stored in `~/.config/tmux-gui/projects.json`.

"Open in terminal" uses `$TERMINAL` if set, otherwise gnome-terminal, kitty, alacritty, wezterm, konsole, xfce4-terminal, tilix, foot, xterm, in that order.

## Build and run
```
cargo build --release
./target/release/tmux-gui
```
Requires `tmux` and `claude` on the PATH.
