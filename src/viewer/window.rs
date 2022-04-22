//! Main application window
//!
//! Hosts the two main sub panes, library and song.
//! Also does fullscreen handling.

use dinoscore::{prelude::*, *};

glib::wrapper! {
	pub struct Window(ObjectSubclass<imp::Window>)
		@extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
		@implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
	pub fn new(app: &Application) -> Self {
		Object::new(&[("application", app)]).expect("Failed to create Window")
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate, Default)]
	#[template(resource = "/de/piegames/dinoscore/viewer/window.ui")]
	pub struct Window {
		#[template_child]
		deck: TemplateChild<adw::Leaflet>,
		#[template_child]
		library: TemplateChild<crate::library_widget::LibraryWidget>,
		#[template_child]
		song: TemplateChild<crate::song_widget::SongWidget>,
		/// When a song is loaded, prevent the screen from going blank
		inhibit_cookie: Cell<Option<u32>>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for Window {
		const NAME: &'static str = "ViewerWindow";
		type Type = super::Window;
		type ParentType = adw::ApplicationWindow;

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	impl ObjectImpl for Window {
		fn constructed(&self, obj: &Self::Type) {
			self.parent_constructed(obj);

			log::debug!("Loading songs");
			let library = Rc::new(RefCell::new(library::Library::load().unwrap()));
			self.song.init(library.clone());
			self.library.init(library, self.song.get());

			/* Fullscreen handling */

			let enter_fullscreen = gio::SimpleAction::new("enter-fullscreen", None);
			obj.add_action(&enter_fullscreen);
			enter_fullscreen.connect_activate(clone!(@weak obj => @default-panic, move |_a, _p| {
				obj.fullscreen();
			}));

			let leave_fullscreen = gio::SimpleAction::new("leave-fullscreen", None);
			leave_fullscreen.set_enabled(false);
			obj.add_action(&leave_fullscreen);
			leave_fullscreen.connect_activate(clone!(@weak obj => @default-panic, move |_a, _p| {
				obj.unfullscreen();
			}));

			let toggle_fullscreen = gio::SimpleAction::new("toggle-fullscreen", None);
			obj.add_action(&toggle_fullscreen);
			toggle_fullscreen.connect_activate(clone!(@weak obj => @default-panic, move |_a, _p| {
				obj.set_fullscreened(!obj.is_fullscreened());
			}));
		}
	}

	impl WidgetImpl for Window {}

	impl WindowImpl for Window {}

	impl ApplicationWindowImpl for Window {}

	impl AdwApplicationWindowImpl for Window {}

	#[gtk::template_callbacks]
	impl Window {
		#[template_callback]
		fn update_song_loaded(&self) {
			let uuid = self.song.property::<Option<String>>("song-id");
			let application = self.instance().application().unwrap();
			if uuid.is_some() {
				self.inhibit_cookie.set(Some(application.inhibit(
					Some(&self.instance()),
					gtk::ApplicationInhibitFlags::IDLE,
					Some("You wouldn't want your screen to go blank while playing an instrument"),
				)));
				self.deck.navigate(adw::NavigationDirection::Forward);
			} else {
				application.uninhibit(self.inhibit_cookie.take().unwrap());
				self.deck.navigate(adw::NavigationDirection::Back);
			}
		}

		#[template_callback]
		fn update_song_title(&self) {
			let obj = self.instance();
			obj.set_title(
				self.song
					.property::<Option<String>>("song-name")
					.as_ref()
					.map(|title| format!("{} – DiNoScore", title))
					.as_deref()
					.or(Some("DiNoScore")),
			);
			self.library.update_side_panel();
		}

		#[template_callback]
		fn fullscreen_changed(&self) {
			let obj = self.instance();
			let fullscreen = obj.is_fullscreened();

			/* This will automatically show and hide the buttons */
			obj.lookup_action("enter-fullscreen")
				.unwrap()
				.downcast::<gio::SimpleAction>()
				.unwrap()
				.set_enabled(!fullscreen);
			obj.lookup_action("leave-fullscreen")
				.unwrap()
				.downcast::<gio::SimpleAction>()
				.unwrap()
				.set_enabled(fullscreen);
			if fullscreen {
				log::debug!("Going fullscreen");
			} else {
				log::debug!("Going unfullscreen");
			}
			obj.queue_draw();
		}
	}
}
