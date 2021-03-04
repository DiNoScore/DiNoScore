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

#[derive(woab::BuilderSignal, Debug)]
pub enum LibrarySignal {
	LoadSong(gtk::IconView, gtk::TreePath),
}

impl actix::StreamHandler<LibrarySignal> for LibraryActor {
	fn handle(&mut self, signal: LibrarySignal, _ctx: &mut Self::Context) {
		match signal {
			LibrarySignal::LoadSong(_library_grid, item) => {
				println!("Loading song:");
				let text = self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&item).unwrap(), 1)
					.get::<glib::GString>()
					.unwrap()
					.unwrap();
				let uuid = self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&item).unwrap(), 2)
					.get::<glib::GString>()
					.unwrap()
					.unwrap();
				dbg!(&text.as_str());
				dbg!(&uuid.as_str());

				self.widgets.deck.navigate(libhandy::NavigationDirection::Forward);

				let uuid = uuid::Uuid::parse_str(uuid.as_str()).unwrap();
				let mut library = self.library.borrow_mut();
				let song = library.songs.get_mut(&uuid).unwrap();
				self.song_actor.try_send(LoadSong {
					meta: song.index.clone(),
					pdf: song.load_sheet(),
				}).unwrap();
			},
		}
	}
}
