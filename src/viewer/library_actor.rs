use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;
use libhandy::prelude::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};
/* Weird that this is required for it to work */
use actix::Actor;
use dinoscore::*;
use libhandy::prelude::HeaderBarExt;
use std::sync::mpsc::*;

use super::song_actor::{LoadSong, SongActor};

pub fn create(
	builder: &woab::BuilderConnector,
	song_actor: actix::Addr<SongActor>,
	library: library::Library,
) -> actix::Addr<LibraryActor> {
	LibraryActor::create(move |_ctx| LibraryActor {
		widgets: builder.widgets().unwrap(),
		application: todo!(),
		library: Rc::new(RefCell::new(library)),
		song_actor,
		reference_time: std::time::SystemTime::now(),
		song_filter: Box::new(|_| true),
	})
}

pub struct LibraryActor {
	pub widgets: LibraryWidgets,
	pub application: gtk::Application,
	pub library: Rc<RefCell<library::Library>>,
	pub song_actor: actix::Addr<SongActor>,
	/**
	 * Our scores decay over time, so we need to fix a point in time for the values to be comparable.
	 * This weakly depends on the assumption that the application won't be running for months, and that
	 * no time traveling or clock fuckery will occur in that order of magnitude.
	 */
	pub reference_time: std::time::SystemTime,
	/** This is a predicate: true = show that song */
	pub song_filter: Box<dyn Fn(&collection::SongMeta) -> bool>,
}

#[derive(woab::WidgetsFromBuilder)]
pub struct LibraryWidgets {
	store_songs: gtk::ListStore,
	library_grid: gtk::IconView,
	deck: libhandy::Deck,
	sidebar_revealer: gtk::Revealer,
	search_entry: gtk::SearchEntry,
}

impl actix::Actor for LibraryActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		use actix::AsyncContext;

		log::info!("Starting LibraryActor");
		/* TODO add a true loading spinner */
		let store_songs = &self.widgets.store_songs;
		// store_songs.set_sort_column_id(gtk::SortColumn::Index(1), gtk::SortType::Ascending);
		store_songs.set_sort_column_id(gtk::SortColumn::Index(3), gtk::SortType::Descending);

		self.reload_songs_filtered();
		self.widgets.library_grid.show();
		self.widgets.library_grid.grab_focus();

		let focus_search = gio::SimpleAction::new("focus_search", None);
		self.application.add_action(&focus_search);
		self.application.set_accels_for_action("app.focus_search", &["<Ctrl>F"]);
		woab::route_action(&focus_search, ctx.address()).unwrap();
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		log::debug!("Library Quit");
		// TODO also this won't work on quit because who's going to wait for that thread to finish?
		// self.library.borrow_mut().save_in_background();
	}
}

impl LibraryActor {
	fn reload_songs_filtered(&self) {
		let library = &self.library.borrow();
		let store_songs = self.widgets.store_songs.clone();
		woab::spawn_outside(async move {
			store_songs.clear();
		});
		for (uuid, song) in library.songs.iter() {
			if (*self.song_filter)(&song.index) {
				/* Add an item with the name and UUID
				* Index, column, value
				* The columns are: thumbnail, title, UUID, usage_score
				*/
				let store_songs = self.widgets.store_songs.clone();
				let row_data: [glib::Value; 4] = [
					song.thumbnail().to_value(),
					song.title().unwrap_or("<no title>").to_value(),
					uuid.to_string().to_value(),
					self.library.borrow().stats[uuid].usage_score(&self.reference_time).to_value(),
				];
				woab::spawn_outside(async move {
					store_songs.set(
						&store_songs.append(),
						&[0, 1, 2, 3],
						&[&row_data[0], &row_data[1], &row_data[2], &row_data[3]],
					);
				});
			}
		}
	}

	fn load_song(&mut self, song: uuid::Uuid) {
		log::info!("Loading song: {}", song);

		let mut library = self.library.borrow_mut();
		library.stats.get_mut(&song).unwrap().on_load();
		library.save_in_background();

		/* Find our song and update it. */
		let store_songs = &self.widgets.store_songs;
		store_songs.foreach(|_model, _path, iter| {
			let uuid2: String = store_songs.get_value(iter, 2).get::<String>().unwrap().unwrap();
			let uuid2: uuid::Uuid = uuid::Uuid::parse_str(&uuid2).unwrap();
			if uuid2 == song {
				store_songs.set_value(
					iter,
					3,
					&library.stats[&song].usage_score(&self.reference_time).to_value(),
				);
				true
			} else {
				false
			}
		});

		let song = library.songs.get_mut(&song).unwrap();

		self.widgets
			.deck
			.navigate(libhandy::NavigationDirection::Forward);

		let song_actor = self.song_actor.clone();
		let mut event = Some(LoadSong {
			meta: song.index.clone(),
			pages: unsafe { unsafe_force::Send::new(song.load_sheets().unwrap()) },
		});
		/* Hack to get the event processed in the correct order */
		glib::timeout_add_local(50, move || {
			song_actor.try_send(event.take().unwrap()).unwrap();
			Continue(false)
		});
	}
}

impl actix::Handler<woab::Signal> for LibraryActor {
	type Result = woab::SignalResult;

	fn handle(&mut self, signal: woab::Signal, _ctx: &mut Self::Context) -> woab::SignalResult {
		signal!(match (signal) {
			"SongSelected" => {
				let song: Option<uuid::Uuid> = {
					self.widgets.library_grid.get_selected_items()
						.into_iter()
						.next() /* There is at most one item */
						.map(|song| self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&song).unwrap(), 2))
						.map(|uuid: glib::Value| uuid.get::<glib::GString>().unwrap().unwrap())
						.map(|uuid| uuid::Uuid::parse_str(uuid.as_str()).unwrap())
				};
				self.widgets.sidebar_revealer.set_reveal_child(song.is_some());
			},
			"PlaySelected" => {
				log::debug!("Activated");
				let uuid = {
					/* There is exactly one item */
					let song = self.widgets.library_grid.get_selected_items().into_iter().next().unwrap();
					let uuid = self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&song).unwrap(), 2);
					uuid::Uuid::parse_str(uuid.get::<glib::GString>().unwrap().unwrap().as_str()).unwrap()
				};
				self.load_song(uuid);
			},
			"LoadSong" => |_ = gtk::IconView, item = gtk::TreePath | {
				let uuid = self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&item).unwrap(), 2)
					.get::<glib::GString>()
					.unwrap()
					.unwrap();
				let uuid = uuid::Uuid::parse_str(uuid.as_str()).unwrap();
				self.load_song(uuid);
			},
			"on_search_entry_changed" => |entry = gtk::SearchEntry| {
				/* TODO use unicase crate instead. And maybe also a fuzzy matcher */
				let query = entry.get_text().to_string().trim().to_lowercase();
				self.song_filter = if query.is_empty() {
					Box::new(|_| true)
				} else {
					Box::new(move |song| {
						song.title
							.as_ref()
							.map(|title| title.trim().to_lowercase().contains(&query))
							.unwrap_or(false)
						|| song.composer
							.as_ref()
							.map(|composer| composer.trim().to_lowercase().contains(&query))
							.unwrap_or(false)
					})
				};
				self.reload_songs_filtered();
			},
			"on_search_entry_next" => {
				let selected = self.widgets.library_grid
					.get_selected_items()
					.into_iter()
					.next()
					.map(|mut path| {path.next(); path})
					.unwrap_or_else(gtk::TreePath::new_first);
				let library_grid = self.widgets.library_grid.clone();
				woab::spawn_outside(async move {
					library_grid.select_path(&selected);
				});
			},
			"on_search_entry_previous" => {
				let selected = self.widgets.library_grid
					.get_selected_items()
					.into_iter()
					.next()
					.map(|mut path| {path.prev(); path})
					.unwrap_or_else(gtk::TreePath::new_first);
				let library_grid = self.widgets.library_grid.clone();
				woab::spawn_outside(async move {
					library_grid.select_path(&selected);
				});
			},
			"on_search_stopped" => {
				self.song_filter = Box::new(|_| true);
				self.reload_songs_filtered();
			},
			"focus_search" => {
				let search_entry = self.widgets.search_entry.clone();
				woab::spawn_outside(async move {
					search_entry.grab_focus();
				});
			}
		});

		Ok(None)
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct UpdateSongUsage {
	pub seconds_elapsed: u64,
	pub song: uuid::Uuid,
}

impl actix::Handler<UpdateSongUsage> for LibraryActor {
	type Result = ();

	fn handle(&mut self, message: UpdateSongUsage, _ctx: &mut Self::Context) {
		self.library.borrow_mut()
			.stats
			.get_mut(&message.song)
			.unwrap()
			.on_update(message.seconds_elapsed);
	}
}
