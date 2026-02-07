# Launcher Keyboard & Controller Navigation

Issue: <https://github.com/aarmea/shepherd-launcher/issues/20>

Summary:
- Added keyboard/controller navigation for the launcher grid (arrow keys, WASD, D-pad) with focus movement between tiles.
- Enabled focus styling on tiles and ensured the grid focuses the first tile on state updates.
- Added keyboard shortcuts to close the launcher (Alt+F4, Ctrl+W, Home).

Key files:
- crates/shepherd-launcher-ui/src/app.rs
- crates/shepherd-launcher-ui/src/grid.rs
- crates/shepherd-launcher-ui/src/tile.rs
