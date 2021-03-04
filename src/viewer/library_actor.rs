use std::sync::Arc;
use std::cell::RefCell;
use std::rc::Rc;
use gtk::prelude::*;
use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use libhandy::prelude::*;
/* Weird that this is required for it to work */
use libhandy::prelude::HeaderBarExt;
use std::sync::mpsc::*;
use dinoscore::*;

use super::song_actor::{SongActor, LoadSong};

pub fn create(builder: &woab::BuilderConnector, song_actor: actix::Addr<SongActor>, library: library::Library) -> actix::Addr<LibraryActor> {
	builder.actor()
		.connect_signals(LibrarySignal::connector())
		.create(|_ctx| {
			LibraryActor {
				widgets: builder.widgets().unwrap(),
				library: Rc::new(RefCell::new(library)),
				song_actor,
			}
		})
}

pub struct LibraryActor {
	widgets: LibraryWidgets,
	library: Rc<RefCell<library::Library>>,
	song_actor: actix::Addr<SongActor>,
}

#[derive(woab::WidgetsFromBuilder)]
struct LibraryWidgets {
	store_songs: gtk::ListStore,
	library_grid: gtk::IconView,
	deck: libhandy::Deck,
	sidebar_revealer: gtk::Revealer,
}

impl actix::Actor for LibraryActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, _ctx: &mut Self::Context) {
		println!("Starting LibraryActor");
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
				]
			);
		}
		self.widgets.library_grid.show();
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		println!("Library Quit");
	}
}

impl LibraryActor {
	fn load_song(&mut self, song: uuid::Uuid) {
		println!("Loading song: {}", song);

		let mut library = self.library.borrow_mut();
		let song = library.songs.get_mut(&song).unwrap();

		self.widgets.deck.navigate(libhandy::NavigationDirection::Forward);

		let song_actor = self.song_actor.clone();
		let mut event = Some(LoadSong {
			meta: song.index.clone(),
			pdf: song.load_sheet(),
		});
		/* Hack to get the event processed in the correct order */
		glib::timeout_add_local(50, move || {
			song_actor.try_send(event.take().unwrap()).unwrap();
			Continue(false)
		});
	}
}

#[derive(woab::BuilderSignal, Debug)]
pub enum LibrarySignal {
	SongSelected(gtk::IconView),
	PlaySelected(gtk::Button),
	LoadSong(gtk::IconView, gtk::TreePath),
}

impl actix::StreamHandler<LibrarySignal> for LibraryActor {
	fn handle(&mut self, signal: LibrarySignal, _ctx: &mut Self::Context) {
		match signal {
			LibrarySignal::SongSelected(_library_grid) => {
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
			LibrarySignal::PlaySelected(_button) => {
				println!("Activated");
				let uuid = {
					/* There is exactly one item */
					let song = self.widgets.library_grid.get_selected_items().into_iter().next().unwrap();
					let uuid = self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&song).unwrap(), 2);
					uuid::Uuid::parse_str(uuid.get::<glib::GString>().unwrap().unwrap().as_str()).unwrap()
				};
				self.load_song(uuid);
			},
			LibrarySignal::LoadSong(_library_grid, item) => {
				let uuid = self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&item).unwrap(), 2)
					.get::<glib::GString>()
					.unwrap()
					.unwrap();
				let uuid = uuid::Uuid::parse_str(uuid.as_str()).unwrap();
				self.load_song(uuid);
			},
		}
	}
}
