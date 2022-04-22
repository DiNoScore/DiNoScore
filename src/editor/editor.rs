use dinoscore::{collection::*, prelude::*, *};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Business logic for the editor

/**
 * Representation of a [`collection::SongFile`] together with its
 * [`SongMeta`](collection::SongMeta) as required by the editor
 */
pub struct EditorSongFile {
	pub pages: Vec<(Rc<RawPageImage>, Vec<Staff>)>,

	pub piece_starts: BTreeMap<StaffIndex, String>,
	pub section_starts: BTreeMap<StaffIndex, SectionMeta>,

	/// A unique identifier for this song that is stable across file modifications
	song_uuid: Uuid,
	/* /// Effectively a random string generated on each save. Useful for caching
	 * version_uuid: Uuid, */
}

impl Default for EditorSongFile {
	fn default() -> Self {
		Self::new()
	}
}

impl EditorSongFile {
	pub fn new() -> Self {
		Self {
			pages: Vec::new(),
			piece_starts: {
				let mut map = BTreeMap::new();
				map.insert(0.into(), "".into());
				map
			},
			section_starts: {
				let mut map = BTreeMap::new();
				map.insert(0.into(), SectionMeta::default());
				map
			},
			song_uuid: Uuid::new_v4(),
		}
	}

	pub fn get_staves(&self) -> TiVec<StaffIndex, Staff> {
		self.pages
			.iter()
			.enumerate()
			.flat_map(|(_page_index, page)| page.1.iter())
			.cloned()
			.collect()
	}

	pub fn get_pages(&self) -> Vec<Rc<RawPageImage>> {
		self.pages.iter().map(|(page, _)| page).cloned().collect()
	}

	pub fn count_staves_before(&self, page: PageIndex) -> usize {
		self.pages[0..*page].iter().map(|p| p.1.len()).sum()
	}

	fn shift_items(&mut self, threshold: usize, offset: isize) {
		/* I whish Rust had generic closures or partially applied functions */
		fn mapper<T: Clone>(
			threshold: usize,
			offset: isize,
		) -> impl Fn((&StaffIndex, &mut T)) -> (StaffIndex, T) {
			move |(&index, value)| {
				if *index > threshold {
					(
						StaffIndex((*index as isize + offset) as usize),
						value.clone(),
					)
				} else {
					(index, value.clone())
				}
			}
		}
		/* TODO replace with `drain_filter` once stabilized */
		self.piece_starts = self
			.piece_starts
			.iter_mut()
			.map(mapper(threshold, offset))
			.collect();
		self.section_starts = self
			.section_starts
			.iter_mut()
			.map(mapper(threshold, offset))
			.collect();
	}

	pub fn add_page(&mut self, page: RawPageImage) {
		self.pages.push((Rc::new(page), vec![]));
	}

	pub fn remove_page(&mut self, page_index: PageIndex) {
		let (_page, staves) = self.pages.remove(*page_index);
		self.shift_items(
			self.count_staves_before(page_index),
			-(staves.len() as isize),
		);
		self.pages[*page_index..]
			.iter_mut()
			.flat_map(|(_page, staves)| staves)
			.for_each(|staff| {
				staff.page -= PageIndex(1);
			});
	}

	pub fn add_staves(&mut self, page_index: PageIndex, staves: Vec<Staff>) {
		self.shift_items(
			self.count_staves_before(page_index) + staves.len(),
			staves.len() as isize,
		);
		self.pages[*page_index].1.extend(staves);
	}

	/// Insert single staff, maintain y ordering
	pub fn add_staff(&mut self, page_index: PageIndex, staff: Staff) -> usize {
		/* Find correct index to insert based on y coordinate */
		let index = self.pages[*page_index]
			.1
			.iter()
			.enumerate()
			.find(|(_, staff2)| staff2.start.1 > staff.start.1)
			.map(|(index, _)| index)
			.unwrap_or_else(|| self.pages[*page_index].1.len());
		self.shift_items(self.count_staves_before(page_index) + index, 1);
		self.pages[*page_index].1.insert(index, staff);
		index
	}

	/** The `staff` parameter is relative to the page index */
	pub fn delete_staff(&mut self, page_index: PageIndex, staff: usize) {
		self.shift_items(self.count_staves_before(page_index) + staff, -1);
		self.pages[*page_index].1.remove(staff);
	}

	/// Move a single staff, update the y ordering
	pub fn move_staff(&mut self, page_index: PageIndex, staff: usize, dx: f64, dy: f64) -> usize {
		self.shift_items(self.count_staves_before(page_index) + staff, -1);
		let mut staff = self.pages[*page_index].1.remove(staff);
		staff.start.0 += dx;
		staff.start.1 += dy;
		staff.end.0 += dx;
		staff.end.1 += dy;
		self.add_staff(page_index, staff)
	}

	pub fn save(&self, file: std::path::PathBuf) -> anyhow::Result<()> {
		let song = SongMeta {
			n_pages: self.pages.len(),
			staves: self.get_staves(),
			piece_starts: self.piece_starts.clone(),
			section_starts: self.section_starts.clone(),
			song_uuid: self.song_uuid,
			version_uuid: uuid::Uuid::new_v4(),
			title: None,
			composer: None,
		};
		use std::ops::Deref;
		let thumbnail =
			SongFile::generate_thumbnail(&song, self.pages.iter().map(|(page, _)| page.deref()))
				.expect("Failed to generate thumbnail");
		SongFile::save(
			file,
			song,
			self.pages.iter().map(|(page, _)| page.deref()),
			thumbnail,
			true, // TODO overwrite?!
		)?;
		Ok(())
	}
}
