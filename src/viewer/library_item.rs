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
		favorite: bool,
	) -> Self {
		Object::builder()
			.property("uuid", uuid.to_string())
			.property("title", title)
			.property("thumbnail", thumbnail)
			.property("score", score)
			.property("favorite", favorite)
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
		#[property(
			get,
			set = |obj: &&LibraryItem, val: bool| obj.set_favorite(val),
			explicit_notify,
		)]
		favorite: RefCell<bool>,
		#[property(get = |obj: &&LibraryItem| obj.get_style_classes())]
		style_classes: std::marker::PhantomData<Vec<String>>,
	}

	impl LibraryItem {
		fn set_favorite(&self, value: bool) {
			if *self.favorite.borrow() == value {
				return;
			}
			*self.favorite.borrow_mut() = value;
			self.obj().notify_favorite();
			self.obj().notify_style_classes();
		}

		fn get_style_classes(&self) -> Vec<String> {
			if *self.favorite.borrow() {
				vec!["library-item".into(), "library-item-favorite".into()]
			} else {
				vec!["library-item".into()]
			}
		}
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
