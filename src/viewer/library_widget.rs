use dinoscore::{prelude::*, *};

glib::wrapper! {
	pub struct LibraryWidget(ObjectSubclass<imp::LibraryWidget>)
		@extends gtk::Box, gtk::Widget,
		@implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl LibraryWidget {
	pub fn init(
		&self,
		library: Rc<RefCell<library::Library>>,
		song: crate::song_widget::SongWidget,
	) {
		self.imp().library.set(library).unwrap();
		self.imp().song.set(song).unwrap();
		self.imp().reload_songs_filtered();
	}

	pub fn update_side_panel(&self) {
		self.imp().on_item_selected();
	}

	#[cfg(test)]
	pub fn select_first_entry(&self) {
		self.imp()
			.library_grid
			.get()
			.select_path(&gtk::TreePath::new_first());
	}

	#[cfg(test)]
	pub fn activate_selected_entry(&self) {
		self.imp().on_play_button_pressed();
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate)]
	#[template(resource = "/de/piegames/dinoscore/viewer/library.ui")]
	pub struct LibraryWidget {
		#[template_child]
		store_songs: TemplateChild<gtk::ListStore>,
		#[template_child]
		pub library_grid: TemplateChild<gtk::IconView>,
		#[template_child]
		sidebar_revealer: TemplateChild<gtk::Revealer>,
		#[template_child]
		search_entry: TemplateChild<gtk::SearchEntry>,

		/* Revealer (when clicked on song) */
		#[template_child]
		stats_times_played: TemplateChild<gtk::Label>,
		#[template_child]
		stats_time_played: TemplateChild<gtk::Label>,
		#[template_child]
		stats_last_played: TemplateChild<gtk::Label>,

		/**
		 * Our scores decay over time, so we need to fix a point in time for the values to be comparable.
		 * This weakly depends on the assumption that the application won't be running for months, and that
		 * no time traveling or clock fuckery will occur in that order of magnitude.
		 */
		reference_time: std::time::SystemTime,
		pub library: OnceCell<Rc<RefCell<library::Library>>>,
		pub song: OnceCell<crate::song_widget::SongWidget>,
		song_filter: RefCell<Box<dyn Fn(&collection::SongMeta) -> bool>>,
	}

	impl Default for LibraryWidget {
		fn default() -> Self {
			LibraryWidget {
				store_songs: Default::default(),
				library_grid: Default::default(),
				sidebar_revealer: Default::default(),
				search_entry: Default::default(),
				reference_time: std::time::SystemTime::now(),
				library: Default::default(),
				song: Default::default(),
				song_filter: RefCell::new(Box::new(|_| true)),
				stats_times_played: Default::default(),
				stats_time_played: Default::default(),
				stats_last_played: Default::default(),
			}
		}
	}

	#[glib::object_subclass]
	impl ObjectSubclass for LibraryWidget {
		const NAME: &'static str = "ViewerLibrary";
		type Type = super::LibraryWidget;
		type ParentType = gtk::Box;

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	impl ObjectImpl for LibraryWidget {
		fn constructed(&self, obj: &Self::Type) {
			self.parent_constructed(obj);

			let store_songs = &self.store_songs;
			/* Sort by usage score */
			store_songs.set_sort_column_id(gtk::SortColumn::Index(3), gtk::SortType::Descending);

			/* Deferring is required for some reason */
			glib::MainContext::default().spawn_local(
				clone!(@weak obj => @default-panic, async move {
					obj.imp().library_grid.grab_focus();
				}),
			);
		}
	}

	impl WidgetImpl for LibraryWidget {}

	impl BoxImpl for LibraryWidget {}

	#[gtk::template_callbacks]
	impl LibraryWidget {
		/// Update the songs list according to our library and the set filter
		pub fn reload_songs_filtered(&self) {
			let library = &self.library.get().unwrap().borrow();
			self.store_songs.clear();
			for (uuid, song) in library.songs.iter() {
				if (*self.song_filter.borrow())(&song.index) {
					/* Add an item with the name and UUID */
					// TODO cleanup once glib::Value implements ToValue
					let thumbnail = song.thumbnail().cloned();
					let title = song.title().unwrap_or("<no title>").to_owned();
					let score = library.stats[uuid].usage_score(&self.reference_time);
					let uuid = uuid.to_string();

					self.store_songs.set(
						&self.store_songs.append(),
						/* The columns are: thumbnail, title, UUID, usage_score */
						&[(0, &thumbnail), (1, &title), (2, &uuid), (3, &score)],
					);
				}
			}
		}

		/// Play a song
		fn load_song(&self, uuid: uuid::Uuid) {
			log::info!("Loading song: {}", uuid);

			let mut library = self.library.get().unwrap().borrow_mut();

			/* Find our song and update it. */
			let store_songs = &self.store_songs;
			store_songs.foreach(|_model, _path, iter| {
				let uuid2: String = store_songs.get().get(iter, 2);
				let uuid2: uuid::Uuid = uuid::Uuid::parse_str(&uuid2).unwrap();
				if uuid2 == uuid {
					store_songs.set_value(
						iter,
						3,
						&library.stats[&uuid]
							.usage_score(&self.reference_time)
							.to_value(),
					);
					true
				} else {
					false
				}
			});

			let song = library.songs.get_mut(&uuid).unwrap();

			let index = song.index.clone();
			let sheet = song.load_sheets().unwrap();
			let scale_mode = library
				.stats
				.get_mut(&uuid)
				.unwrap()
				.scale_options
				.as_ref()
				.copied()
				.unwrap_or_default();
			std::mem::drop(library);
			self.song.get().unwrap().load_song(index, sheet, scale_mode);
		}

		#[template_callback]
		pub fn on_item_selected(&self) {
			let song: Option<uuid::Uuid> = {
				self.library_grid
					.selected_items()
					.into_iter()
					.next() /* There is at most one item */
					.map(|song| {
						self.store_songs
							.get()
							.get::<glib::GString>(&self.store_songs.iter(&song).unwrap(), 2)
					})
					.map(|uuid| uuid::Uuid::parse_str(uuid.as_str()).unwrap())
			};

			if let Some(song) = song {
				let library = self.library.get().unwrap().borrow();
				let stats = library.stats.get(&song).unwrap();
				self.stats_times_played
					.set_label(&stats.times_played.to_string());
				self.stats_time_played
					.set_label(&format!("{:.1}", stats.seconds_played as f64 / 3600.0));
				self.stats_last_played.set_label(
					&stats
						.last_played
						.and_then(|last_played| {
							last_played
								.duration_since(std::time::SystemTime::UNIX_EPOCH)
								.ok()
						})
						.and_then(|last_played| {
							glib::DateTime::from_unix_local(last_played.as_secs() as i64).ok()
						})
						.and_then(|last_played| last_played.format("%_x").ok())
						.unwrap_or_else(|| "never".into()),
				);
			}

			self.sidebar_revealer.set_reveal_child(song.is_some());
		}

		/// A song entry from the IconView was activated through double-click or enter
		#[template_callback]
		fn on_item_activated(&self, item: &gtk::TreePath) {
			let uuid = self
				.store_songs
				.get()
				.get::<glib::GString>(&self.store_songs.iter(item).unwrap(), 2);
			let uuid = uuid::Uuid::parse_str(uuid.as_str()).unwrap();
			self.load_song(uuid);
		}

		/// The "play" button that appears when selecting a song was pressed
		#[template_callback]
		pub(super) fn on_play_button_pressed(&self) {
			log::debug!("Activated");
			let uuid = {
				/* There is exactly one item */
				let song = self
					.library_grid
					.selected_items()
					.into_iter()
					.next()
					.unwrap();
				let uuid = self
					.store_songs
					.get()
					.get::<glib::GString>(&self.store_songs.iter(&song).unwrap(), 2);
				uuid::Uuid::parse_str(uuid.as_str()).unwrap()
			};
			self.load_song(uuid);
		}

		#[template_callback]
		fn on_search_entry_changed(&self, entry: &gtk::SearchEntry) {
			/* TODO use unicase crate instead. And maybe also a fuzzy matcher */
			let query = entry.text().to_string().trim().to_lowercase();
			*self.song_filter.borrow_mut() = if query.is_empty() {
				Box::new(|_| true)
			} else {
				Box::new(move |song| {
					song.title
						.as_ref()
						.map(|title| title.trim().to_lowercase().contains(&query))
						.unwrap_or(false) || song
						.composer
						.as_ref()
						.map(|composer| composer.trim().to_lowercase().contains(&query))
						.unwrap_or(false)
				})
			};
			self.reload_songs_filtered();
		}

		#[template_callback]
		fn on_search_entry_next(&self) {
			let selected = self
				.library_grid
				.selected_items()
				.into_iter()
				.next()
				.map(|mut path| {
					path.next();
					path
				})
				.unwrap_or_else(gtk::TreePath::new_first);
			let library_grid = self.library_grid.clone();
			library_grid.select_path(&selected);
		}

		#[template_callback]
		fn on_search_entry_previous(&self) {
			let selected = self
				.library_grid
				.selected_items()
				.into_iter()
				.next()
				.map(|mut path| {
					path.prev();
					path
				})
				.unwrap_or_else(gtk::TreePath::new_first);
			let library_grid = self.library_grid.clone();
			library_grid.select_path(&selected);
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
