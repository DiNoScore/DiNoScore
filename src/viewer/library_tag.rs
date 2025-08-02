use dinoscore::{prelude::*, *};

glib::wrapper! {
	pub struct LibraryTag(ObjectSubclass<imp::LibraryTag>)
		@extends gtk::ToggleButton, gtk::Button, gtk::Widget,
		@implements gtk::Accessible, gtk::Buildable, gtk::Actionable, gtk::ConstraintTarget, gtk::Native, gtk::Root;
}

impl LibraryTag {
	pub fn new(kind: String, value: String) -> Self {
		Object::builder()
			.property("kind", &kind)
			.property("value", &value)
			.build()
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate, Properties, Default)]
	#[properties(wrapper_type = super::LibraryTag)]
	#[template(resource = "/de/piegames/dinoscore/viewer/library_tag.ui")]
	pub struct LibraryTag {
		#[property(get, set)]
		kind: RefCell<Option<String>>,
		#[property(get, set)]
		value: RefCell<Option<String>>,
		#[property(get, set)]
		count: Cell<u32>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for LibraryTag {
		const NAME: &'static str = "ViewerLibraryTag";
		type Type = super::LibraryTag;
		type ParentType = gtk::ToggleButton;

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	#[glib::derived_properties]
	impl ObjectImpl for LibraryTag {
		fn constructed(&self) {
			self.parent_constructed();
			let _obj = self.obj();
		}
	}

	impl WidgetImpl for LibraryTag {}
	impl ButtonImpl for LibraryTag {}
	impl ToggleButtonImpl for LibraryTag {}

	#[gtk::template_callbacks]
	impl LibraryTag {
		#[template_callback]
		fn on_kind_changed(&self) {
			let obj = self.obj();
			obj.css_classes()
				.retain_mut(|class| !class.starts_with("tag-"));
			if let Some(kind) = obj.kind() {
				obj.add_css_class(&format!("tag-{kind}"));
			}
		}

		#[template_callback]
		fn update_content(&self) {
			let obj = self.obj();
			let count = self.count.get();
			obj.set_sensitive(count > 0);
			obj.set_label(
				&self
					.value
					.borrow()
					.as_ref()
					.map(|value| format!("{value} ({count})"))
					.unwrap_or_default(),
			);
		}
	}
}
