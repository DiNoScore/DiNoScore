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
		let obj = $this.instance().downgrade();
		move |$($arg),*| {
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

pub use image_util::{PageImage, PageImageBox, RawPageImage};

pub fn create_progress_bar_dialog(text: &str) -> (gtk::Dialog, gtk::ProgressBar) {
	let progress = gtk::Dialog::new();
	progress.set_modal(true);
	// progress.set_skip_taskbar_hint(true);
	progress.set_destroy_with_parent(true);
	// progress.set_position(gtk::WindowPosition::CenterOnParent);
	let bar = gtk::ProgressBar::new();
	bar.set_show_text(true);
	bar.set_text(Some(text));
	// progress.content_area().add(&bar);
	// progress.set_title("Loading…");
	progress.set_deletable(false);
	// progress.show_all();
	bar.set_fraction(0.0);
	(progress, bar)
}

/// Commonly used imports
pub mod prelude {
	pub use adw::{prelude::*, subclass::prelude::*};
	pub use glib::subclass::{object::*, prelude::*, types::*};
	pub use gtk::{
		gdk, gdk_pixbuf, gio, glib,
		glib::{clone, prelude::*},
		graphene, gsk,
		prelude::*,
		subclass::prelude::*,
		CompositeTemplate, TemplateChild,
	};
	pub use gtk4 as gtk;
	pub use libadwaita as adw;

	pub use typed_index_collections::TiVec;

	pub use glib::Object;
	pub use gtk::Application;
	pub use once_cell::unsync::{Lazy, OnceCell};
	pub use std::{
		cell::{Cell, RefCell},
		rc::Rc,
		sync::Arc,
	};
}
