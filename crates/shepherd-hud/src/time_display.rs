//! Time display widget
//!
//! Shows elapsed time, remaining time, or countdown.

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use std::cell::RefCell;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct TimeDisplay {
        pub label: RefCell<Option<gtk4::Label>>,
        pub total_secs: RefCell<Option<u64>>,
        pub remaining_secs: RefCell<Option<u64>>,
        pub paused: RefCell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TimeDisplay {
        const NAME: &'static str = "ShepherdTimeDisplay";
        type Type = super::TimeDisplay;
        type ParentType = gtk4::Box;
    }

    impl ObjectImpl for TimeDisplay {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.set_orientation(gtk4::Orientation::Horizontal);
            obj.set_spacing(4);

            // Time icon
            let icon = gtk4::Image::from_icon_name("preferences-system-time-symbolic");
            icon.set_pixel_size(20);
            obj.append(&icon);

            // Time label
            let label = gtk4::Label::new(Some("--:--"));
            label.add_css_class("time-display");
            obj.append(&label);

            *self.label.borrow_mut() = Some(label);
        }
    }

    impl WidgetImpl for TimeDisplay {}
    impl BoxImpl for TimeDisplay {}
}

glib::wrapper! {
    pub struct TimeDisplay(ObjectSubclass<imp::TimeDisplay>)
        @extends gtk4::Box, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl TimeDisplay {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Set the time limit in seconds
    pub fn set_time_limit(&self, total_secs: Option<u64>) {
        let imp = self.imp();
        *imp.total_secs.borrow_mut() = total_secs;
        self.update_display();
    }

    /// Set the remaining time in seconds
    pub fn set_remaining(&self, remaining_secs: Option<u64>) {
        let imp = self.imp();
        *imp.remaining_secs.borrow_mut() = remaining_secs;
        self.update_display();
    }

    /// Set paused state
    pub fn set_paused(&self, paused: bool) {
        let imp = self.imp();
        *imp.paused.borrow_mut() = paused;
        self.update_display();
    }

    /// Update the display based on current state
    fn update_display(&self) {
        let imp = self.imp();

        if let Some(label) = imp.label.borrow().as_ref() {
            let remaining = *imp.remaining_secs.borrow();
            let paused = *imp.paused.borrow();

            let text = if let Some(secs) = remaining {
                let formatted = format_duration(secs);
                if paused {
                    format!("{} ⏸", formatted)
                } else {
                    formatted
                }
            } else {
                "--:--".to_string()
            };

            label.set_text(&text);

            // Update styling based on remaining time
            label.remove_css_class("time-warning");
            label.remove_css_class("time-critical");

            if let Some(secs) = remaining {
                if secs <= 60 {
                    label.add_css_class("time-critical");
                } else if secs <= 300 {
                    label.add_css_class("time-warning");
                }
            }
        }
    }
}

impl Default for TimeDisplay {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a duration in seconds as HH:MM:SS or MM:SS
fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "00:00");
        assert_eq!(format_duration(59), "00:59");
        assert_eq!(format_duration(60), "01:00");
        assert_eq!(format_duration(3599), "59:59");
        assert_eq!(format_duration(3600), "01:00:00");
        assert_eq!(format_duration(3661), "01:01:01");
    }
}
