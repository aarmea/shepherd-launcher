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
            obj.set_can_focus(true);
            obj.set_focusable(true);

            // Configure flow box
            self.flow_box.set_homogeneous(true);
            self.flow_box.set_selection_mode(gtk4::SelectionMode::None);
            self.flow_box.set_max_children_per_line(6);
            self.flow_box.set_min_children_per_line(2);
            self.flow_box.set_row_spacing(24);
            self.flow_box.set_column_spacing(24);
            self.flow_box.set_halign(gtk4::Align::Center);
            self.flow_box.set_valign(gtk4::Align::Center);
            self.flow_box.set_hexpand(true);
            self.flow_box.set_vexpand(true);
            self.flow_box.add_css_class("launcher-grid");

            // Add keyboard activation for when FlowBox has focus
            let flow_box_key_controller = gtk4::EventControllerKey::new();
            let on_launch_clone = self.on_launch.clone();
            let flow_box_weak = self.flow_box.downgrade();
            flow_box_key_controller.connect_key_pressed(move |_, key, _, _| {
                if key == gtk4::gdk::Key::Return || key == gtk4::gdk::Key::space {
                    if let Some(flow_box) = flow_box_weak.upgrade() {
                        // Get the currently focused child (FlowBoxChild wrapper)
                        if let Some(focus_child) = flow_box.focus_child() {
                            // FlowBox wraps children in FlowBoxChild, so get the actual child
                            if let Some(flow_box_child) = focus_child.downcast_ref::<gtk4::FlowBoxChild>() {
                                if let Some(child) = flow_box_child.child() {
                                    if let Ok(tile) = child.downcast::<super::LauncherTile>() {
                                        if let Some(entry_id) = tile.entry_id()
                                            && let Some(callback) = on_launch_clone.borrow().as_ref() {
                                                callback(entry_id);
                                                return glib::Propagation::Stop;
                                            }
                                    }
                                }
                            }
                        }
                    }
                }
                glib::Propagation::Proceed
            });
            self.flow_box.add_controller(flow_box_key_controller);

            // Add arrow key capture to start navigation without Tab
            let arrow_key_controller = gtk4::EventControllerKey::new();
            arrow_key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
            let flow_box_weak_arrow = self.flow_box.downgrade();
            arrow_key_controller.connect_key_pressed(move |_, key, _, _| {
                // Check if an arrow key was pressed
                if matches!(key, gtk4::gdk::Key::Up | gtk4::gdk::Key::Down | gtk4::gdk::Key::Left | gtk4::gdk::Key::Right) {
                    if let Some(flow_box) = flow_box_weak_arrow.upgrade() {
                        // If the flow box doesn't have focus, grab it
                        if !flow_box.has_focus() {
                            flow_box.grab_focus();
                        }
                    }
                }
                glib::Propagation::Proceed
            });
            obj.add_controller(arrow_key_controller);

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

            // Connect keyboard activation (Enter/Space)
            let key_controller = gtk4::EventControllerKey::new();
            let on_launch_key = imp.on_launch.clone();
            let tile_weak = tile.downgrade();
            key_controller.connect_key_pressed(move |_, key, _, _| {
                if key == gtk4::gdk::Key::Return || key == gtk4::gdk::Key::space {
                    if let Some(tile) = tile_weak.upgrade() {
                        if let Some(entry_id) = tile.entry_id()
                            && let Some(callback) = on_launch_key.borrow().as_ref() {
                                callback(entry_id);
                            }
                    }
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            tile.add_controller(key_controller);

            imp.flow_box.insert(&tile, -1);
            imp.tiles.borrow_mut().push(tile);
        }

        // Automatically grab focus when entries are set
        self.grab_focus();
    }

    /// Enable or disable all tiles
    pub fn set_tiles_sensitive(&self, sensitive: bool) {
        for tile in self.imp().tiles.borrow().iter() {
            tile.set_sensitive(sensitive);
        }
    }
}

impl Default for LauncherGrid {
    fn default() -> Self {
        Self::new()
    }
}
