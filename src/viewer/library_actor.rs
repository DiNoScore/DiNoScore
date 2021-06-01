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
		library: Rc::new(RefCell::new(library)),
		song_actor,
	})
}

pub struct LibraryActor {
	pub widgets: LibraryWidgets,
	pub library: Rc<RefCell<library::Library>>,
	pub song_actor: actix::Addr<SongActor>,
}

#[derive(woab::WidgetsFromBuilder)]
pub struct LibraryWidgets {
	store_songs: gtk::ListStore,
	library_grid: gtk::IconView,
	deck: libhandy::Deck,
	sidebar_revealer: gtk::Revealer,
}

impl actix::Actor for LibraryActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, _ctx: &mut Self::Context) {
		log::info!("Starting LibraryActor");
		/* TODO add a true loading spinner */
		let library = &self.library;
		let store_songs = &self.widgets.store_songs;
		store_songs.set_sort_column_id(gtk::SortColumn::Index(1), gtk::SortType::Ascending);

		for (_uuid, song) in library.borrow().songs.iter() {
			// TODO clean this up
			/* Add an item with the name and UUID
			 * Index, column, value
			 * The columns are: thumbnail, title, UUID
			 */
			store_songs.set(
				&store_songs.append(),
				&[0, 1, 2],
				&[
					&song.thumbnail(),
					&song.title().unwrap_or("<no title>").to_value(),
					&song.uuid().to_string().to_value(),
				],
			);
		}
		self.widgets.library_grid.show();
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		log::debug!("Library Quit");
		self.library.borrow_mut().save_in_background();
	}
}

impl LibraryActor {
	fn load_song(&mut self, song: uuid::Uuid) {
		log::info!("Loading song: {}", song);

		let mut library = self.library.borrow_mut();
		library.stats.get_mut(&song).unwrap().on_load();
		library.save_in_background();
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
