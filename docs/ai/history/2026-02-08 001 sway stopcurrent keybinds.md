# Sway StopCurrent Keybinds

Prompt summary:
- Move keyboard exit handling out of launcher UI code and into `sway.conf`.
- Keep controller home behavior as-is.
- Ensure "exit" uses the API (`StopCurrent`) rather than closing windows directly.

Implemented summary:
- Added a `--stop-current` mode to `shepherd-launcher` that sends `StopCurrent` to shepherdd over IPC and exits.
- Added Sway keybindings for `Alt+F4`, `Ctrl+W`, and `Home` that execute `shepherd-launcher --stop-current`.
- Kept controller home behavior in launcher UI unchanged.

Key files:
- `sway.conf`
- `crates/shepherd-launcher-ui/src/main.rs`
