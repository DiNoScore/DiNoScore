//! A row widget for displaying a part preview in the song preview pane

use dinoscore::prelude::*;

glib::wrapper! {
	pub struct PartPreviewRow(ObjectSubclass<imp::PartPreviewRow>)
		@extends gtk::ListBoxRow, gtk::Widget,
		@implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}

impl PartPreviewRow {
	pub fn new(name: &str) -> Self {
		Object::builder().property("name", name).build()
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate, Properties, Default)]
	#[properties(wrapper_type = super::PartPreviewRow)]
	#[template(resource = "/de/piegames/dinoscore/viewer/part_preview_row.ui")]
	pub struct PartPreviewRow {
		#[template_child]
		picture: TemplateChild<gtk::Picture>,
		#[template_child]
		label: TemplateChild<gtk::Label>,

		#[property(get, set)]
		name: RefCell<String>,
		#[property(get, set, nullable)]
		paintable: RefCell<Option<gdk::Paintable>>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for PartPreviewRow {
		const NAME: &'static str = "PartPreviewRow";
		type Type = super::PartPreviewRow;
		type ParentType = gtk::ListBoxRow;

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	#[glib::derived_properties]
	impl ObjectImpl for PartPreviewRow {
		fn constructed(&self) {
			self.parent_constructed();
			/* Set a placeholder so the layout has a stable size before images load */
			self.picture
				.set_paintable(Some(&gdk::Paintable::new_empty(400, 100)));
		}
	}

	impl WidgetImpl for PartPreviewRow {}
	impl ListBoxRowImpl for PartPreviewRow {}

	#[gtk::template_callbacks]
	impl PartPreviewRow {
		#[template_callback]
		fn on_name_changed(&self) {
			let name = self.name.borrow();
			self.label.set_label(&name);
			self.label.set_visible(!name.is_empty());
			self.picture.set_alternative_text(Some(&name));
		}

		#[template_callback]
		fn on_paintable_changed(&self) {
			self.picture.set_paintable(self.paintable.borrow().as_ref());
		}
	}
}
