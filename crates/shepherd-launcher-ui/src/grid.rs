//! Grid widget containing launcher tiles

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use shepherd_api::EntryView;
use shepherd_util::EntryId;
use std::cell::RefCell;
use std::rc::Rc;

use crate::tile::LauncherTile;

mod imp {
    use super::*;

    type LaunchCallback = Rc<RefCell<Option<Box<dyn Fn(EntryId) + 'static>>>>;

    pub struct LauncherGrid {
        pub flow_box: gtk4::FlowBox,
        pub tiles: RefCell<Vec<LauncherTile>>,
        pub on_launch: LaunchCallback,
    }

    impl Default for LauncherGrid {
        fn default() -> Self {
            Self {
                flow_box: gtk4::FlowBox::new(),
                tiles: RefCell::new(Vec::new()),
                on_launch: Rc::new(RefCell::new(None)),
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

            // Configure flow box
            self.flow_box.set_homogeneous(true);
            self.flow_box.set_selection_mode(gtk4::SelectionMode::Single);
            self.flow_box.set_max_children_per_line(6);
            self.flow_box.set_min_children_per_line(2);
            self.flow_box.set_row_spacing(24);
            self.flow_box.set_column_spacing(24);
            self.flow_box.set_halign(gtk4::Align::Center);
            self.flow_box.set_valign(gtk4::Align::Center);
            self.flow_box.set_hexpand(true);
            self.flow_box.set_vexpand(true);
            self.flow_box.set_focusable(true);
            self.flow_box.add_css_class("launcher-grid");

            // Wrap in a scrolled window
            let scrolled = gtk4::ScrolledWindow::new();
            scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
            scrolled.set_child(Some(&self.flow_box));
            scrolled.set_hexpand(true);
            scrolled.set_vexpand(true);

            obj.append(&scrolled);
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

        self.select_first();
    }

    /// Enable or disable all tiles
    pub fn set_tiles_sensitive(&self, sensitive: bool) {
        for tile in self.imp().tiles.borrow().iter() {
            tile.set_sensitive(sensitive);
        }
    }

    pub fn select_first(&self) {
        let imp = self.imp();
        if let Some(child) = imp.flow_box.child_at_index(0) {
            imp.flow_box.select_child(&child);
            child.grab_focus();
        }
    }

    pub fn move_selection(&self, dx: i32, dy: i32) {
        let imp = self.imp();
        if imp.tiles.borrow().is_empty() {
            return;
        }

        let current_child = imp
            .flow_box
            .selected_children()
            .first()
            .cloned()
            .or_else(|| imp.flow_box.child_at_index(0));
        let Some(current_child) = current_child else {
            return;
        };

        let current_alloc = current_child.allocation();
        let current_x = current_alloc.x();
        let current_y = current_alloc.y();
        let mut best: Option<(gtk4::FlowBoxChild, i32, i32)> = None;

        let tile_count = imp.tiles.borrow().len() as i32;
        for idx in 0..tile_count {
            let Some(candidate) = imp.flow_box.child_at_index(idx) else {
                continue;
            };
            if candidate == current_child {
                continue;
            }

            let alloc = candidate.allocation();
            let x = alloc.x();
            let y = alloc.y();

            let is_direction_match = match (dx, dy) {
                (-1, 0) => y == current_y && x < current_x,
                (1, 0) => y == current_y && x > current_x,
                (0, -1) => y < current_y,
                (0, 1) => y > current_y,
                _ => false,
            };
            if !is_direction_match {
                continue;
            }

            let primary_dist = match (dx, dy) {
                (-1, 0) | (1, 0) => (x - current_x).abs(),
                (0, -1) | (0, 1) => (y - current_y).abs(),
                _ => i32::MAX,
            };
            let secondary_dist = match (dx, dy) {
                (-1, 0) | (1, 0) => (y - current_y).abs(),
                (0, -1) | (0, 1) => (x - current_x).abs(),
                _ => i32::MAX,
            };

            let replace = match best {
                None => true,
                Some((_, best_primary, best_secondary)) => {
                    primary_dist < best_primary
                        || (primary_dist == best_primary && secondary_dist < best_secondary)
                }
            };

            if replace {
                best = Some((candidate, primary_dist, secondary_dist));
            }
        }

        if let Some((child, _, _)) = best {
            imp.flow_box.select_child(&child);
            child.grab_focus();
        }
    }

    pub fn launch_selected(&self) {
        let imp = self.imp();
        let maybe_child = imp.flow_box.selected_children().first().cloned();
        let Some(child) = maybe_child else {
            return;
        };

        let index = child.index();
        if index < 0 {
            return;
        }

        let tile = imp.tiles.borrow().get(index as usize).cloned();
        if let Some(tile) = tile {
            if !tile.is_sensitive() {
                return;
            }
            if let Some(entry_id) = tile.entry_id()
                && let Some(callback) = imp.on_launch.borrow().as_ref()
            {
                callback(entry_id);
            }
        }
    }

}

impl Default for LauncherGrid {
    fn default() -> Self {
        Self::new()
    }
}
