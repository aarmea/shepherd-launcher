# Controller And Keyboard Launching

Issue: <https://github.com/aarmea/shepherd-launcher/issues/20>

Prompt summary:
- Launching activities required pointer input.
- Requested non-pointer controls:
  - Selection via arrow keys, WASD, D-pad, or analog stick
  - Launch via Enter, Space, controller A/B/Start
  - Exit via Alt+F4, Ctrl+W, controller home
- Goal was better accessibility and support for pointer-less handheld systems.

Implemented summary:
- Added keyboard navigation and activation support in launcher UI grid.
- Added explicit keyboard exit shortcuts at the window level.
- Added gamepad input handling via `gilrs` for D-pad, analog stick, A/B/Start launch, and home exit.
- Added focused tile styling so non-pointer selection is visible.

Key files:
- crates/shepherd-launcher-ui/src/app.rs
- crates/shepherd-launcher-ui/src/grid.rs
- crates/shepherd-launcher-ui/Cargo.toml
