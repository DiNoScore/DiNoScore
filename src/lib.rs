#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
	cell::RefCell,
	collections::{BTreeMap, HashMap, HashSet},
	ops::RangeInclusive,
	path::{Path, PathBuf},
	rc::Rc,
};

use futures::prelude::*;

use adw::prelude::*;
use gtk::{gdk, gio, glib, glib::clone, prelude::*};
use gtk4 as gtk;
use libadwaita as adw;

use uuid::Uuid;

/// clone_self
#[macro_export]
macro_rules! clone_ {
	($this:expr, move |$obj:tt, $($arg:tt),*| $body:block ) => ({
		let obj = $this.obj().downgrade();
		move |$($arg),*| {
			let $obj = obj.upgrade().expect("Failed to upgrade `self`");
			$body
		}
	});
	($this:expr, move |$obj:tt| $body:block ) => ({
		let obj = $this.obj().downgrade();
		move || {
			let $obj = obj.upgrade().expect("Failed to upgrade `self`");
			$body
		}
	});
}

/// Stolen from https://docs.rs/try-block/0.1.0/src/try_block/lib.rs.html#22-29
#[macro_export]
macro_rules! catch {
    { $token:expr } => {
        (|| $token)()
    }
}

pub mod collection;
pub mod image_util;
pub mod layout;
pub mod library;
#[cfg(feature = "editor")]
pub mod recognition;
pub mod unsafe_force;

pub use image_util::PageImage;

pub fn create_progress_bar_dialog(
	text: &str,
	parent: &impl IsA<gtk::Widget>,
) -> (adw::Dialog, gtk::ProgressBar) {
	let dialog = adw::Dialog::builder().title("Loading…").build();

	let content = gtk::Box::builder()
		.orientation(gtk::Orientation::Vertical)
		.spacing(12)
		.margin_top(24)
		.margin_bottom(24)
		.margin_start(24)
		.margin_end(24)
		.build();

	let bar = gtk::ProgressBar::new();
	bar.set_show_text(true);
	bar.set_text(Some(text));
	content.append(&bar);
	dialog.set_child(Some(&content));

	dialog.present(Some(parent));
	bar.set_fraction(0.0);
	(dialog, bar)
}

pub fn create_progress_spinner_dialog(text: &str, parent: &impl IsA<gtk::Widget>) -> adw::Dialog {
	let dialog = adw::Dialog::builder().title("Loading…").build();

	let content = gtk::Box::builder()
		.orientation(gtk::Orientation::Vertical)
		.spacing(12)
		.margin_top(24)
		.margin_bottom(24)
		.margin_start(24)
		.margin_end(24)
		.build();

	let spinner = gtk::Spinner::new();
	spinner.set_spinning(true);
	content.append(&spinner);
	content.append(&gtk::Label::new(Some(text)));
	dialog.set_child(Some(&content));

	dialog.present(Some(parent));
	dialog
}

/// Commonly used imports
pub mod prelude {
	pub use adw::{prelude::*, subclass::prelude::*};
	pub use glib::subclass::{object::*, prelude::*, types::*};
	pub use gtk::{
		gdk, gio, glib,
		glib::{clone, prelude::*},
		graphene, gsk,
		prelude::*,
		subclass::prelude::*,
		CompositeTemplate, TemplateChild,
	};
	pub use gtk4 as gtk;
	pub use libadwaita as adw;

	pub use typed_index_collections::{TiSlice, TiVec};

	pub use glib::{Object, Properties};
	pub use gtk::Application;
	pub use std::{
		cell::{Cell, OnceCell, RefCell},
		rc::Rc,
		sync::Arc,
	};
}
