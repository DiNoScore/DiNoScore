//! Glib object for the list model in the library view

#[allow(unused_imports)]
use dinoscore::{prelude::*, *};

glib::wrapper! {
	pub struct LibraryItem(ObjectSubclass<imp::LibraryItem>);
}

impl LibraryItem {
	pub fn new(
		uuid: &uuid::Uuid,
		title: String,
		thumbnail: Option<&gdk::Texture>,
		score: f64,
	) -> Self {
		Object::builder()
			.property("uuid", uuid.to_string())
			.property("title", title)
			.property("thumbnail", thumbnail)
			.property("score", score)
			.build()
	}

	pub fn uuid(&self) -> uuid::Uuid {
		uuid::Uuid::parse_str(&*self.imp().uuid.borrow()).unwrap()
	}
}

mod imp {
	use super::*;

	#[derive(Properties, Default)]
	#[properties(wrapper_type = super::LibraryItem)]
	pub struct LibraryItem {
		#[property(set)]
		pub(super) uuid: RefCell<String>,
		#[property(get, set)]
		title: RefCell<String>,
		#[property(get, set)]
		thumbnail: RefCell<Option<gdk::Texture>>,
		#[property(get, set)]
		score: RefCell<f64>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for LibraryItem {
		const NAME: &'static str = "ViewerLibraryItem";
		type Type = super::LibraryItem;
		type ParentType = glib::Object;
	}

	#[glib::derived_properties]
	impl ObjectImpl for LibraryItem {}
}
