/*! The library of songs managed by the user.
 *
 * This does not contain the actual song files themselves, but instead all the usage metadata.
 * Nevertheless, this is the primary thing a user manages and of uttermost importance.
 */
use super::*;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{serde_as, DisplayFromStr};
use std::collections::HashMap;
use uuid::Uuid;

pub enum Song {}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibrarySong {
	pub song: Uuid,
	pub times_played: u32,
	pub seconds_played: u64,
	pub last_played: Option<std::time::SystemTime>,
	pub usage_score: f32,
}

impl LibrarySong {
	pub fn new(song: Uuid) -> Self {
		LibrarySong {
			song,
			times_played: 0,
			seconds_played: 0,
			last_played: None,
			usage_score: 1.0,
		}
	}

	pub fn on_load(&mut self) {
		self.times_played += 1;
		self.last_played = Some(std::time::SystemTime::now());
	}

	pub fn on_update(&mut self, add_seconds: u64) {
		self.seconds_played += add_seconds;
		self.last_played = Some(std::time::SystemTime::now());
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "version")]
enum LibraryFile {
	#[serde(rename = "0")]
	V0 { songs: HashMap<Uuid, LibrarySong> },
}

#[derive(Debug)]
pub struct Library {
	pub songs: HashMap<Uuid, collection::SongFile>,
	pub stats: HashMap<Uuid, LibrarySong>,
}

impl Library {
	pub fn load() -> Result<Self, ()> {
		// TODO don't hardcode here
		let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
		let songs = collection::load();
		let mut stats: HashMap<Uuid, LibrarySong> = {
			match xdg.find_data_file("library.json") {
				Some(path) => {
					let stats: LibraryFile =
						serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
					match stats {
						LibraryFile::V0 { songs } => songs,
					}
				},
				None => HashMap::new(),
			}
		};
		/* Create stats for all new songs */
		for uuid in songs.keys() {
			if stats.contains_key(uuid) {
				continue;
			}
			stats.insert(*uuid, LibrarySong::new(*uuid));
		}
		Ok(Library { songs, stats })
	}
}
