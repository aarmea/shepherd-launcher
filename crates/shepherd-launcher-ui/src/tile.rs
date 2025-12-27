//! Individual tile widget for the launcher grid

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use shepherd_api::EntryView;
use std::cell::RefCell;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LauncherTile {
        pub entry: RefCell<Option<EntryView>>,
        pub icon: gtk4::Image,
        pub label: gtk4::Label,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LauncherTile {
        const NAME: &'static str = "ShepherdLauncherTile";
        type Type = super::LauncherTile;
        type ParentType = gtk4::Button;
    }

    impl ObjectImpl for LauncherTile {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();

            // Create layout
            let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
            content.set_halign(gtk4::Align::Center);
            content.set_valign(gtk4::Align::Center);

            // Icon
            self.icon.set_pixel_size(96);
            self.icon.set_icon_name(Some("application-x-executable"));
            content.append(&self.icon);

            // Label
            self.label.set_wrap(true);
            self.label.set_wrap_mode(gtk4::pango::WrapMode::Word);
            self.label.set_justify(gtk4::Justification::Center);
            self.label.set_max_width_chars(12);
            self.label.add_css_class("tile-label");
            content.append(&self.label);

            obj.set_child(Some(&content));
            obj.add_css_class("launcher-tile");
            obj.set_size_request(160, 160);
        }
    }

    impl WidgetImpl for LauncherTile {}
    impl ButtonImpl for LauncherTile {}
}

glib::wrapper! {
    pub struct LauncherTile(ObjectSubclass<imp::LauncherTile>)
        @extends gtk4::Button, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl LauncherTile {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_entry(&self, entry: EntryView) {
        let imp = self.imp();

        // Set label
        imp.label.set_text(&entry.label);

        // Set icon
        if let Some(ref icon_ref) = entry.icon_ref {
            // Try to use the icon reference as an icon name
            imp.icon.set_icon_name(Some(icon_ref));
        } else {
            // Default icon based on entry kind
            let icon_name = match entry.kind_tag {
                shepherd_api::EntryKindTag::Process => "application-x-executable",
                shepherd_api::EntryKindTag::Vm => "computer",
                shepherd_api::EntryKindTag::Media => "video-x-generic",
                shepherd_api::EntryKindTag::Custom => "applications-other",
            };
            imp.icon.set_icon_name(Some(icon_name));
        }

        // Entry is available if enabled and has no blocking reasons
        let available = entry.enabled && entry.reasons.is_empty();
        self.set_sensitive(available);

        // Add tooltip with reason if not available
        if !available && !entry.reasons.is_empty() {
            // Format the first reason for tooltip
            let reason_text = format!("{:?}", entry.reasons[0]);
            self.set_tooltip_text(Some(&reason_text));
        } else {
            self.set_tooltip_text(None);
        }

        *imp.entry.borrow_mut() = Some(entry);
    }

    pub fn entry(&self) -> Option<EntryView> {
        self.imp().entry.borrow().clone()
    }

    pub fn entry_id(&self) -> Option<shepherd_util::EntryId> {
        self.imp().entry.borrow().as_ref().map(|e| e.entry_id.clone())
    }
}

impl Default for LauncherTile {
    fn default() -> Self {
        Self::new()
    }
}
