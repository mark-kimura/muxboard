# tmux-gui

A small desktop window for the everyday tmux operations, so you never have to remember the prefix-key shortcuts.

## Layout
- **Left**: a tree of sessions with their windows nested underneath. A green filled dot means the session is open in a terminal window; a small triangle marks each session's active window.
- **Right**: the selected session or window, one primary button ("Open in terminal"), an "Actions" menu with everything else, a live view of the terminal text refreshed every second, and a command box underneath.

## How to do things
- Click a session or window to select it. Double-click to open it in a terminal.
- Right-click any row for its actions. The same actions are in the "Actions" menu on the right.
- Session actions: open in terminal, new window, rename, close terminals (keep running), kill.
- Window actions: open in terminal here, make active, rename, kill.
- "+ New session" is above the tree. Killing anything asks for confirmation first.
- Command box: type a command line and press Enter. It is typed into the selected window followed by Enter, and the result shows in the live view. For anything interactive, open the terminal.

"Open in terminal" uses `$TERMINAL` if set, otherwise gnome-terminal, kitty, alacritty, wezterm, konsole, xfce4-terminal, tilix, foot, xterm, in that order.

## Build and run
```
cargo build --release
./target/release/tmux-gui
```
Requires `tmux` on the PATH.
