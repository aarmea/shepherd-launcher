//! Grid widget containing launcher tiles

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use shepherd_api::EntryView;
use shepherd_util::EntryId;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::input::NavCommand;
use crate::tile::LauncherTile;

mod imp {
    use super::*;

    type LaunchCallback = Rc<RefCell<Option<Box<dyn Fn(EntryId) + 'static>>>>;

    pub struct LauncherGrid {
        pub flow_box: gtk4::FlowBox,
        pub scrolled: gtk4::ScrolledWindow,
        pub tiles: RefCell<Vec<LauncherTile>>,
        pub on_launch: LaunchCallback,
        pub selected_index: Cell<Option<usize>>,
        pub columns: Cell<u32>,
    }

    impl Default for LauncherGrid {
        fn default() -> Self {
            Self {
                flow_box: gtk4::FlowBox::new(),
                scrolled: gtk4::ScrolledWindow::new(),
                tiles: RefCell::new(Vec::new()),
                on_launch: Rc::new(RefCell::new(None)),
                selected_index: Cell::new(None),
                columns: Cell::new(6), // Will be updated based on actual layout
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LauncherGrid {
        const NAME: &'static str = "ShepherdLauncherGrid";
        type Type = super::LauncherGrid;
        type ParentType = gtk4::Box;
    }

    impl ObjectImpl for LauncherGrid {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.set_orientation(gtk4::Orientation::Vertical);
            obj.set_halign(gtk4::Align::Fill);
            obj.set_valign(gtk4::Align::Fill);
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.set_focusable(true);
            obj.set_focus_on_click(true);

            // Configure flow box
            self.flow_box.set_homogeneous(true);
            self.flow_box.set_selection_mode(gtk4::SelectionMode::Single);
            self.flow_box.set_activate_on_single_click(false);
            self.flow_box.set_max_children_per_line(6);
            self.flow_box.set_min_children_per_line(2);
            self.flow_box.set_row_spacing(24);
            self.flow_box.set_column_spacing(24);
            self.flow_box.set_halign(gtk4::Align::Center);
            self.flow_box.set_valign(gtk4::Align::Center);
            self.flow_box.set_hexpand(true);
            self.flow_box.set_vexpand(true);
            self.flow_box.add_css_class("launcher-grid");
            self.flow_box.set_focusable(true);

            // Wrap in a scrolled window
            self.scrolled
                .set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
            self.scrolled.set_child(Some(&self.flow_box));
            self.scrolled.set_hexpand(true);
            self.scrolled.set_vexpand(true);

            obj.append(&self.scrolled);
        }
    }

    impl WidgetImpl for LauncherGrid {}
    impl BoxImpl for LauncherGrid {}
}

glib::wrapper! {
    pub struct LauncherGrid(ObjectSubclass<imp::LauncherGrid>)
        @extends gtk4::Box, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl LauncherGrid {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Set the callback for when an entry is launched
    pub fn connect_launch<F: Fn(EntryId) + 'static>(&self, callback: F) {
        *self.imp().on_launch.borrow_mut() = Some(Box::new(callback));
    }

    /// Update the grid with new entries
    pub fn set_entries(&self, entries: Vec<EntryView>) {
        let imp = self.imp();

        // Clear existing tiles
        while let Some(child) = imp.flow_box.first_child() {
            imp.flow_box.remove(&child);
        }
        imp.tiles.borrow_mut().clear();
        imp.selected_index.set(None);

        // Create tiles for enabled entries
        for entry in entries {
            // Skip disabled entries
            if !entry.enabled {
                continue;
            }

            let tile = LauncherTile::new();
            tile.set_entry(entry);

            // Connect click handler
            let on_launch = imp.on_launch.clone();
            tile.connect_clicked(move |tile| {
                if let Some(entry_id) = tile.entry_id()
                    && let Some(callback) = on_launch.borrow().as_ref() {
                        callback(entry_id);
                    }
            });

            imp.flow_box.insert(&tile, -1);
            imp.tiles.borrow_mut().push(tile);
        }

        // Select first tile if we have any
        if !imp.tiles.borrow().is_empty() {
            self.select_index(0);
        }
    }

    /// Enable or disable all tiles
    pub fn set_tiles_sensitive(&self, sensitive: bool) {
        for tile in self.imp().tiles.borrow().iter() {
            tile.set_sensitive(sensitive);
        }
    }

    /// Handle a navigation command
    pub fn handle_nav_command(&self, command: NavCommand) {
        let imp = self.imp();
        let tiles = imp.tiles.borrow();
        let count = tiles.len();

        if count == 0 {
            return;
        }

        let current = imp.selected_index.get().unwrap_or(0);
        let columns = self.get_columns_count();

        match command {
            NavCommand::Up => {
                if current >= columns {
                    self.select_index(current - columns);
                }
            }
            NavCommand::Down => {
                let next = current + columns;
                if next < count {
                    self.select_index(next);
                }
            }
            NavCommand::Left => {
                if current > 0 {
                    self.select_index(current - 1);
                }
            }
            NavCommand::Right => {
                if current + 1 < count {
                    self.select_index(current + 1);
                }
            }
            NavCommand::Activate => {
                drop(tiles);
                self.activate_selected();
            }
        }
    }

    /// Select a tile by index
    fn select_index(&self, index: usize) {
        let imp = self.imp();
        let tiles = imp.tiles.borrow();

        if index >= tiles.len() {
            return;
        }

        // Remove selected class from previous tile
        if let Some(prev_idx) = imp.selected_index.get()
            && let Some(prev_tile) = tiles.get(prev_idx)
        {
            prev_tile.remove_css_class("selected");
        }

        // Add selected class to new tile
        if let Some(tile) = tiles.get(index) {
            tile.add_css_class("selected");
            tile.grab_focus();

            // Scroll to make sure the tile is visible
            if let Some(child) = imp.flow_box.child_at_index(index as i32) {
                imp.flow_box.select_child(&child);

                // Ensure the tile is scrolled into view
                let adj = imp.scrolled.vadjustment();
                let (_, y) = tile.translate_coordinates(&imp.scrolled, 0.0, 0.0).unwrap_or((0.0, 0.0));
                let tile_height = tile.height() as f64;
                let view_height = imp.scrolled.height() as f64;

                if y < 0.0 {
                    adj.set_value(adj.value() + y - 24.0);
                } else if y + tile_height > view_height {
                    adj.set_value(adj.value() + (y + tile_height - view_height) + 24.0);
                }
            }
        }

        imp.selected_index.set(Some(index));
    }

    /// Activate the currently selected tile
    fn activate_selected(&self) {
        let imp = self.imp();

        if let Some(index) = imp.selected_index.get() {
            let tiles = imp.tiles.borrow();
            if let Some(tile) = tiles.get(index)
                && tile.is_sensitive()
                && let Some(entry_id) = tile.entry_id()
                && let Some(callback) = imp.on_launch.borrow().as_ref()
            {
                callback(entry_id);
            }
        }
    }

    /// Get the current column count based on layout
    fn get_columns_count(&self) -> usize {
        let imp = self.imp();
        let tiles = imp.tiles.borrow();

        if tiles.is_empty() {
            return 1;
        }

        // Try to determine columns from actual layout
        // by checking how many tiles are on the first row (same y position)
        if tiles.len() >= 2 {
            let first_y = tiles[0].allocation().y();
            let mut columns = 1;
            for tile in tiles.iter().skip(1) {
                if tile.allocation().y() == first_y {
                    columns += 1;
                } else {
                    break;
                }
            }
            imp.columns.set(columns as u32);
            return columns;
        }

        imp.columns.get() as usize
    }

    /// Get the flow box for focus management
    pub fn flow_box(&self) -> &gtk4::FlowBox {
        &self.imp().flow_box
    }

    /// Ensure a tile is selected (for when grid becomes visible)
    pub fn ensure_selection(&self) {
        let imp = self.imp();
        if imp.selected_index.get().is_none() && !imp.tiles.borrow().is_empty() {
            self.select_index(0);
        }
    }
}

impl Default for LauncherGrid {
    fn default() -> Self {
        Self::new()
    }
}
