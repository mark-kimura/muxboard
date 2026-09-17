# Muxboard

A desktop window that lists your projects and starts or manages a tmux session in each one, running the terminal program of your choice (Claude Code by default). No tmux keyboard shortcuts needed.

## Layout
By default the window is a narrow project list. A tab on its right edge opens the preview panel and widens the window; clicking the tab again hides the panel and shrinks the window back.

- **Left**: your registered projects. A project is a folder. If a tmux session was started in that folder, the project shows it: a green dot means a terminal window is open on it, a yellow dot means it runs with no terminal open, and a dimmed name with a faint ring means nothing is running. Windows of a running session are nested under the project. tmux sessions that belong to no project are listed under "Other tmux sessions".
- **Right**: the selected project, one primary button, an "Actions" menu, a live view of the terminal text refreshed every second, and a command box underneath.

## How to do things
- "+ Add project" opens a folder picker. The project name defaults to the folder name.
- Ordering: right-click the empty space under the list to choose. "Status" (default) puts projects with a terminal open first, then running ones, then idle ones, each group in the order added. "Name" sorts alphabetically. "Manual" keeps the stored order and lets you drag rows to reorder.
- Double-click a project that is not running: a tmux session is created in its folder, the start command is typed into it, and a terminal opens on it. The default start command is `claude --continue`; change it with the gear button (Settings), or per project in "Edit project". A blank command gives a plain shell, so the app works for any terminal program (vim, htop, ...).
- Double-click a running project (or window): a terminal opens on it.
- Right-click any row for its actions. The same actions are in the "Actions" menu on the right.
- Project actions: start session (or, when running: open in terminal, new window, close terminals, kill session), open folder in the file manager, edit project (name, folder, start command), remove from list. Renaming a project also renames its running session.
- Window actions: open in terminal here, make active, rename, kill.
- Command box: type a command line and press Enter. It is typed into the selected window followed by Enter. For anything interactive, open the terminal.
- Killing anything asks for confirmation first. Removing a project only edits the list; the folder and any session are untouched.

The project list is stored in `~/.config/muxboard/projects.json` and the settings in `~/.config/muxboard/settings.json`.

"Open in terminal" uses the terminal program from Settings if set, else `$TERMINAL` if set, otherwise gnome-terminal, kitty, alacritty, wezterm, konsole, xfce4-terminal, tilix, foot, xterm, in that order.

## Build and run
```
cargo build --release
./target/release/muxboard
```
Requires `tmux` and `claude` on the PATH.
