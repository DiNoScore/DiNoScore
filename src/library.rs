/*! The library of songs managed by the user.
 * 
 * This does not contain the actual song files themselves, but instead all the usage metadata.
 * Nevertheless, this is the primary thing a user manages and of uttermost importance.
 */
use super::*;
use std::collections::HashMap;
use uuid::Uuid;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{serde_as, DisplayFromStr};

pub enum Song {}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibrarySong {
	pub song: Uuid,
	pub times_played: u32,
	pub seconds_played: u32,
	pub last_played: Option<std::time::SystemTime>,
	pub usage_score: f32,
}

impl LibrarySong {
	fn new(song: Uuid) -> Self {
		LibrarySong {
			song,
			times_played: 0,
			seconds_played: 0,
			last_played: None,
			usage_score: 1.0,
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "version")]
enum LibraryFile {
	#[serde(rename = "0")]
	V0 {
		songs: HashMap<Uuid, LibrarySong>,
	},
}

#[derive(Debug)]
pub struct Library {
	pub songs: HashMap<Uuid, collection::SongFile>,
	pub stats: HashMap<Uuid, LibrarySong>,
}

impl Library {
	pub async fn load() -> Result<Self, ()> {
		// TODO don't hardcode here
		let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
		let songs = collection::load().await;
		let mut stats: HashMap<Uuid, LibrarySong> = async_std::task::spawn_blocking(move || {
			match xdg.find_data_file("library.json") {
				Some(path) => {
					let stats: LibraryFile = serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
					match stats {
						LibraryFile::V0 {songs} => songs
					}
				},
				None => HashMap::new()
			}
		}).await;
		/* Create stats for all new songs */
		for uuid in songs.keys() {
			if stats.contains_key(uuid) {
				continue;
			}
			stats.insert(*uuid, LibrarySong::new(*uuid));
		}
		Ok(Library {
			songs,
			stats,
		})
	}

	pub async fn load_song(
		&self,
		name: &str,
		image_cache: Rc<RefCell<lru_disk_cache::LruDiskCache>>,
	) -> Song {
		unimplemented!()
		// Song::new(self.songs.get(name).unwrap(), image_cache).await
	}
}
