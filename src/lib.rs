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
use gtk::prelude::*;

use uuid::Uuid;

/// Stolen from https://docs.rs/try-block/0.1.0/src/try_block/lib.rs.html#22-29
#[macro_export]
macro_rules! catch {
    { $token:expr } => {
        (|| $token)()
    }
}

#[macro_export]
macro_rules! first_arg {
	($signal:expr, $var:ident: $type:ty) => {
		let signal: &woab::Signal<_> = &$signal;
		let $var: $type = signal.param(0)?;
	};
}

#[macro_export]
macro_rules! some_arg {
	($signal:expr, $index:expr, _) => {};
	($signal:expr, $index:expr) => {};
	($signal:expr, $index:expr, _ = $type:ty) => {};
	($signal:expr, $index:expr, $var:pat = $type:ty) => {
		let $var: $type = $signal.param($index)?;
	};
}

#[macro_export]
macro_rules! all_args {
	($signal:expr $(, $var:ident: $type:ty)* $(,)?) => {
		#[allow(unused_variables)]
		let ($($var, )*) = {
			let signal: &woab::Signal<_> = &$signal;
			let index = 0;
			$(
				let $var: $type = signal.param(index)?;
				let index = index + 1;
			)*
			($($var, )*)
		};
	};
	($signal:expr $(, $var:pat $(= $type:ty)?)* $(,)?) => {
		let signal: &woab::Signal<_> = &$signal;
		/* This is not hygienic and will put index into scope */
		let mut index = 0;
		$(
			some_arg!(signal, index $(, $var = $type)?);
			index += 1;
		)*
		/* Make sure we at least can't accidentally use it, creating subtly wrong behavior */
		// std::mem::drop(index);
	};
}

#[macro_export]
macro_rules! parse_args {
	($signal:expr, ..) => {
	};
	($signal:expr, $($var:ident: $type:ty),+) => {
		let signal: &woab::Signal<_> = &$signal;
		/* This is not hygienic and will put index into scope */
		let mut index = 0;
		$(
			let $var: $type = signal.param(index)?;
			index += 1;
		)+
		/* Make sure we at least can't accidentally use it, creating subtly wrong behavior */
		std::mem::drop(index);
	};
	($signal:expr, _$(, $var:ident: $type:ty)*) => {
		let signal: &woab::Signal<_> = &$signal;
		/* This is not hygienic and will put index into scope */
		let mut index = 1;
		$(
			let $var: $type = signal.param(index)?;
			index += 1;
		)+
		/* Make sure we at least can't accidentally use it, creating subtly wrong behavior */
		std::mem::drop(index);
	};
}

#[macro_export]
macro_rules! signal {
	(match ($signal:expr) {
		$( $handler:pat => $(|$($arg:pat $(= $type:ty)?),* $(,)?|)? $content:block ),*
		$(,)?
	}) => {
		let signal: &woab::Signal<_> = &$signal;
		match signal.name() {
			$($handler => {
				$(all_args!(signal $(, $arg $(= $type)?)*);)?
				$content
			}),*
			other => unreachable!("Invalid signal name '{}'", other),
		}
	};
}

pub mod collection;
pub mod layout;
pub mod library;
pub mod page_image;
#[cfg(feature = "editor")]
pub mod recognition;
pub mod unsafe_force;

/// This is a workaround until there is a proper type alias upstream
pub mod cair {
	pub type Result<T> = std::result::Result<T, gtk::cairo::Error>;
}

pub use page_image::{PageImage, PageImageBox, RawPageImage};

pub fn create_progress_bar_dialog(text: &str) -> (gtk::Dialog, gtk::ProgressBar) {
	let progress = gtk::Dialog::new();
	progress.set_modal(true);
	progress.set_skip_taskbar_hint(true);
	progress.set_destroy_with_parent(true);
	progress.set_position(gtk::WindowPosition::CenterOnParent);
	let bar = gtk::ProgressBar::new();
	bar.set_show_text(true);
	bar.set_text(Some(text));
	progress.content_area().add(&bar);
	progress.set_title("Loading…");
	progress.set_deletable(false);
	progress.show_all();
	bar.set_fraction(0.0);
	(progress, bar)
}
