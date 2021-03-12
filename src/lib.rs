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
use gdk::prelude::*;
use gio::prelude::*;
use gtk::prelude::*;

use uuid::Uuid;

// pub mod song;
// use song::*;

pub mod library;
pub mod collection;
pub mod layout;
pub mod owned;
#[cfg(feature = "editor")]
pub mod recognition;

pub fn create_progress_bar_dialog(text: &str) -> (gtk::Dialog, gtk::ProgressBar) {
	let progress = gtk::Dialog::new();
	progress.set_modal(true);
	progress.set_skip_taskbar_hint(true);
	progress.set_destroy_with_parent(true);
	progress.set_position(gtk::WindowPosition::CenterOnParent);
	let bar = gtk::ProgressBar::new();
	bar.set_show_text(true);
	bar.set_text(Some(text));
	progress.get_content_area().add(&bar);
	progress.set_title("Loading…");
	progress.set_deletable(false);
	progress.show_all();
	(progress, bar)
}
