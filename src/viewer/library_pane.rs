use dinoscore::{prelude::*, *};

glib::wrapper! {
	pub struct LibraryPane(ObjectSubclass<imp::LibraryPane>)
		@extends gtk::Box, gtk::Widget,
		@implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl LibraryPane {
	pub fn init(&self, library: Rc<RefCell<library::Library>>, song: crate::song_pane::SongPane) {
		self.imp().library.set(library.clone()).unwrap();
		self.imp().song.set(song).unwrap();
		self.imp().reload_songs_filtered();
		self.imp().side_bar.get().init(library, self.clone());
		self.imp().library_grid.scroll_to(
			0,
			gtk::ListScrollFlags::SELECT | gtk::ListScrollFlags::FOCUS,
			None,
		);
	}

	/* Called when leaving a song to update the statistics */
	pub fn update_side_panel(&self) {
		self.imp().on_item_selected();
	}

	pub fn load_song(&self, song: uuid::Uuid, start_at_part: u32) {
		self.imp().load_song(song, start_at_part);
	}

	#[cfg(test)]
	pub fn select_first_entry(&self) {
		self.imp().library_grid.scroll_to(
			0,
			gtk::ListScrollFlags::SELECT | gtk::ListScrollFlags::FOCUS,
			None,
		);
		self.imp().on_item_selected();
	}

	#[cfg(test)]
	pub fn activate_selected_entry(&self, start_at_part: u32) {
		let song: uuid::Uuid = {
			self.imp()
				.library_grid
				.model()
				.and_downcast_ref::<gtk::SingleSelection>()
				.and_then(gtk::SingleSelection::selected_item)
				.and_downcast_ref::<crate::library_item::LibraryItem>()
				.map(crate::library_item::LibraryItem::uuid)
				.expect("No entry was selected")
		};

		self.load_song(song, start_at_part);
	}
}

mod imp {
	use super::*;

	const SORT_FUN: fn(&glib::Object, &glib::Object) -> std::cmp::Ordering = |l, r| {
		std::cmp::PartialOrd::partial_cmp(&l.property::<f64>("score"), &r.property::<f64>("score"))
			.expect("score can't be inf or NaN")
			.reverse()
	};

	#[derive(CompositeTemplate)]
	#[template(resource = "/de/piegames/dinoscore/viewer/library.ui")]
	pub struct LibraryPane {
		#[template_child]
		pub store_songs: TemplateChild<gio::ListStore>,
		#[template_child]
		pub library_grid: TemplateChild<gtk::GridView>,
		#[template_child]
		search_entry: TemplateChild<gtk::SearchEntry>,
		/* Revealer (when clicked on song) */
		#[template_child]
		pub side_bar: TemplateChild<crate::song_preview::SongPreview>,

		/**
		 * Our scores decay over time, so we need to fix a point in time for the values to be comparable.
		 * This weakly depends on the assumption that the application won't be running for months, and that
		 * no time traveling or clock fuckery will occur in that order of magnitude.
		 */
		reference_time: std::time::SystemTime,
		pub library: OnceCell<Rc<RefCell<library::Library>>>,
		pub song: OnceCell<crate::song_pane::SongPane>,
		song_filter: RefCell<Box<dyn Fn(&collection::SongMeta) -> bool>>,
	}

	impl Default for LibraryPane {
		fn default() -> Self {
			LibraryPane {
				store_songs: Default::default(),
				library_grid: Default::default(),
				search_entry: Default::default(),
				side_bar: Default::default(),
				reference_time: std::time::SystemTime::now(),
				library: Default::default(),
				song: Default::default(),
				song_filter: RefCell::new(Box::new(|_| true)),
			}
		}
	}

	#[glib::object_subclass]
	impl ObjectSubclass for LibraryPane {
		const NAME: &'static str = "ViewerLibrary";
		type Type = super::LibraryPane;
		type ParentType = gtk::Box;

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	impl ObjectImpl for LibraryPane {
		fn constructed(&self) {
			self.parent_constructed();
			let obj = self.obj();

			/* Deferring is required for some reason */
			glib::MainContext::default().spawn_local(clone!(
				#[weak]
				obj,
				async move {
					obj.imp().library_grid.grab_focus();
				}
			));
		}
	}

	impl WidgetImpl for LibraryPane {}

	impl BoxImpl for LibraryPane {}

	#[gtk::template_callbacks]
	impl LibraryPane {
		/// Update the songs list according to our library and the set filter
		pub fn reload_songs_filtered(&self) {
			let library = &self.library.get().unwrap().borrow();
			self.store_songs.remove_all();
			for (uuid, song) in library.songs.iter() {
				if (*self.song_filter.borrow())(&song.index) {
					/* Add an item with the name and UUID */
					let thumbnail = song.thumbnail();
					let title = song.title().unwrap_or("<no title>").to_owned();
					let score = library.stats[uuid].usage_score(&self.reference_time);
					let favorite = library.stats[uuid].favorite;

					self.store_songs.insert_sorted(
						&crate::library_item::LibraryItem::new(uuid, title, thumbnail, score, favorite),
						SORT_FUN,
					);
				}
			}
		}

		/// Play a song
		pub fn load_song(&self, uuid: uuid::Uuid, start_at_part: u32) {
			log::info!("Loading song: {}", uuid);

			let mut library = self.library.get().unwrap().borrow_mut();

			/* Find our song in the UI and update its usage score. */
			let store_songs = &*self.store_songs;
			if let Some(item) = store_songs
				.find_with_equal_func(|item| {
					item.downcast_ref::<crate::library_item::LibraryItem>()
						.unwrap()
						.uuid() == uuid
				})
				.map(|idx| {
					store_songs
						.item(idx)
						.and_downcast::<crate::library_item::LibraryItem>()
						.unwrap()
				}) {
				item.set_score(&library.stats[&uuid].usage_score(&self.reference_time));
				store_songs.sort(SORT_FUN);
			}

			let song = library.songs.get_mut(&uuid).unwrap();

			let index = song.index.clone();
			let sheets = song.load_sheets();
			let scale_mode = library
				.stats
				.get_mut(&uuid)
				.unwrap()
				.scale_options
				.as_ref()
				.copied()
				.unwrap_or_default();
			std::mem::drop(library);
			self.song
				.get()
				.unwrap()
				.load_song(index, sheets, scale_mode, start_at_part);
		}

		#[template_callback]
		pub fn on_item_selected(&self) {
			let song: Option<crate::library_item::LibraryItem> = {
				self.library_grid
					.model()
					.and_downcast_ref::<gtk::SingleSelection>()
					.and_then(gtk::SingleSelection::selected_item)
					.and_downcast::<crate::library_item::LibraryItem>()
			};

			if let Some(song) = song.as_ref() {
				self.side_bar.on_item_selected(song);
			}

			self.side_bar.set_sensitive(song.is_some());
		}

		/// A song entry from the IconView was activated through double-click or enter
		#[template_callback]
		fn on_item_activated(&self, item: u32) {
			let uuid = self
				.store_songs
				.item(item)
				.and_downcast_ref::<crate::library_item::LibraryItem>()
				.unwrap()
				.uuid();
			self.load_song(uuid, 0);
		}

		#[template_callback]
		fn on_search_entry_changed(&self, entry: &gtk::SearchEntry) {
			/* TODO use unicase crate instead. And maybe also a fuzzy matcher */
			let query = entry.text().to_string().trim().to_lowercase();
			*self.song_filter.borrow_mut() = if query.is_empty() {
				Box::new(|_| true)
			} else {
				Box::new(move |song| {
					query
						.split(" ")
						.map(|word| word.trim())
						.filter(|word| !word.is_empty())
						.all(|word| {
							song.title
								.as_ref()
								.map(|title| title.trim().to_lowercase().contains(word))
								.unwrap_or(false) || song
								.composer
								.as_ref()
								.map(|composer| composer.trim().to_lowercase().contains(word))
								.unwrap_or(false)
						})
				})
			};
			self.reload_songs_filtered();
		}

		#[template_callback]
		fn on_search_stopped(&self) {
			*self.song_filter.borrow_mut() = Box::new(|_| true);
			self.reload_songs_filtered();
		}
	}
}

// 	fn stopped(&mut self, _ctx: &mut Self::Context) {
// 		log::debug!("Library Quit");
// 		// TODO also this won't work on quit because who's going to wait for that thread to finish?
// 		// self.library.borrow_mut().save_in_background();
// 	}
// }
