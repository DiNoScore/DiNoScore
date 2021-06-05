/*! The library of songs managed by the user.
 *
 * This does not contain the actual song files themselves, but instead all the usage metadata.
 * Nevertheless, this is the primary thing a user manages and of uttermost importance.
 */
use super::*;
use anyhow::Context;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{serde_as, DisplayFromStr};
use std::{
	collections::HashMap,
	ops::{Add, Neg, Sub},
	time::*,
};
use uuid::Uuid;

pub enum Song {}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibrarySong {
	pub song: Uuid,
	pub times_played: u32,
	pub seconds_played: u64,
	pub last_played: Option<SystemTime>,
	/**
	 * We're using this formula for frecency: https://wiki.mozilla.org/User:Jesse/NewFrecency
	 * The timestamp indicates the moment in time at which the score will reach exactly 1. Due
	 * to the decay factor being constant, this is sufficient to uniquely determine the score at
	 * any point in time.
	 * New songs start with a score of 1, which means `now`.
	 */
	usage_score: SystemTime,
	pub scale_options: Option<ScaleMode>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ScaleMode {
	FitStaves(u32),
	FitPages(u32),
	Zoom(f32),
}

impl Default for ScaleMode {
	fn default() -> Self {
		Self::Zoom(1.0)
	}
}

impl ScaleMode {
	pub fn action_string(&self) -> &'static str {
		match self {
			Self::FitStaves(_) => "fit-staves",
			Self::FitPages(_) => "fit-columns",
			Self::Zoom(_) => "manual",
		}
	}
}

impl LibrarySong {
	/**
	 * The exponential decay factor for the usage score. This corresponding
	 * to all scores halving every month when using one second as unit.
	 */
	const DECAY_FACTOR: f64 = std::f64::consts::LN_2 / (30.0 * 24.0 * 3600.0);

	pub fn new(song: Uuid) -> Self {
		LibrarySong {
			song,
			times_played: 0,
			seconds_played: 0,
			last_played: None,
			usage_score: SystemTime::now(),
			scale_options: None,
		}
	}

	pub fn on_load(&mut self) {
		let now = SystemTime::now();

		self.times_played += 1;
		/* Add 5.0 to the score */
		self.usage_score = Self::usage_score_to_timestamp(self.usage_score(&now) + 5.0, &now);
		self.last_played = Some(now);
	}

	pub fn on_update(&mut self, add_seconds: u64) {
		self.seconds_played += add_seconds;
		// Don't forget to update usage_score when adding that line back in
		//self.last_played = Some(SystemTime::now());
	}

	fn usage_score_to_timestamp(score: f64, now: &SystemTime) -> SystemTime {
		let t = f64::ln(score) / Self::DECAY_FACTOR;
		/* This could be less ugly with negative durations, for example from the Chrono crate */
		if t >= 0.0 {
			(*now).add(Duration::from_secs_f64(t))
		} else {
			(*now).sub(Duration::from_secs_f64(t.abs()))
		}
	}

	/*  */
	pub fn usage_score(&self, now: &SystemTime) -> f64 {
		let t = self
			.usage_score
			.duration_since(*now)
			.map(|d| d.as_secs_f64())
			.or_else(|_|
				/* If one is not before the other, then the other is before the one. */
				now.duration_since(self.usage_score)
					.map(|d| d.as_secs_f64().neg()))
			.unwrap();
		f64::exp(Self::DECAY_FACTOR * t)
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "version")]
enum LibraryFile<'a> {
	#[serde(rename = "0")]
	V0 {
		songs: maybe_owned::MaybeOwned<'a, HashMap<Uuid, LibrarySong>>,
	},
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
						LibraryFile::V0 { songs } => songs.into_owned(),
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

	/* Spawning a background thread is reasonably safe because our file operations are atomic.
	 * Our own worry is if a background write is very slow and finishes after some later ones,
	 * overwriting the file with older data. But eeh.
	 * TODO maybe a mutex would help? And also maybe debounce?
	 * TODO also this won't work on quit because who's going to wait for that thread to finish?
	 */
	pub fn save_in_background(&self) {
		let stats = self.stats.clone();
		std::thread::spawn(move || {
			// TODO don't hardcode here
			let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
			let path = xdg.place_data_file("library.json").unwrap();
			log::info!("Saving database file ({})", path.display());
			let file = atomicwrites::AtomicFile::new(path, atomicwrites::AllowOverwrite);
			file.write(|file| {
				serde_json::to_writer_pretty(
					file,
					&LibraryFile::V0 {
						songs: stats.into(),
					},
				)
			})
			.context("Could not save database (library.json)")
			.unwrap();
		});
	}
}
