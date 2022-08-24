use dinoscore::{collection::*, prelude::*, *};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Business logic for the editor

/**
 * Representation of a [`collection::SongFile`] together with its
 * [`SongMeta`](collection::SongMeta) as required by the editor
 */
pub struct EditorSongFile {
	pub pages: Vec<(Arc<PageImage>, Vec<Staff>)>,

	pub piece_starts: BTreeMap<StaffIndex, String>,
	pub section_starts: BTreeMap<StaffIndex, SectionMeta>,

	/// A unique identifier for this song that is stable across file modifications
	pub song_uuid: Uuid,
	/* /// Effectively a random string generated on each save. Useful for caching
	 * version_uuid: Uuid, */
	pub song_name: String,
	pub song_composer: String,
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
			song_name: "".into(),
			song_composer: "".into(),
		}
	}

	pub fn get_staves(&self) -> TiVec<StaffIndex, Staff> {
		self.pages
			.iter()
			.flat_map(|page| page.1.iter())
			.cloned()
			.collect()
	}

	pub fn get_pages(&self) -> Vec<Arc<PageImage>> {
		self.pages.iter().map(|(page, _)| page).cloned().collect()
	}

	pub fn count_staves_before(&self, page: PageIndex) -> usize {
		self.pages[0..*page].iter().map(|p| p.1.len()).sum()
	}

	fn ensure_invariants(&mut self) {
		if !self.piece_starts.contains_key(&StaffIndex(0)) {
			self.piece_starts.insert(StaffIndex(0), "".into());
			if !self.section_starts.contains_key(&StaffIndex(0)) {
				self.section_starts
					.insert(StaffIndex(0), SectionMeta::default());
			}
		}
		let total_staves: usize = self.pages.iter().map(|p| p.1.len()).sum();
		assert!(**self.piece_starts.keys().next_back().unwrap() < total_staves);
		assert!(**self.section_starts.keys().next_back().unwrap() < total_staves);
	}

	/* Range is start inclusive, end exclusive */
	fn shift_items(&mut self, start: usize, end: Option<usize>, offset: isize) {
		if offset == 0 {
			return;
		}

		/* I whish Rust had generic closures or partially applied functions */
		fn mapper<T: Clone>(
			start: usize,
			end: Option<usize>,
			offset: isize,
		) -> impl Fn((&StaffIndex, &mut T)) -> (StaffIndex, T) {
			move |(&index, value)| {
				if *index >= start && end.map(|end| *index < end).unwrap_or(true) {
					(
						StaffIndex((*index as isize + offset) as usize),
						value.clone(),
					)
				} else {
					(index, value.clone())
				}
			}
		}
		self.piece_starts = self
			.piece_starts
			.iter_mut()
			.map(mapper(start, end, offset))
			.collect();
		self.section_starts = self
			.section_starts
			.iter_mut()
			.map(mapper(start, end, offset))
			.collect();
	}

	pub fn add_page(&mut self, page: PageImage) {
		self.pages.push((Arc::new(page), vec![]));
	}

	pub fn remove_page(&mut self, page_index: PageIndex) {
		let staves = self.get_staves();
		if !staves.is_empty() {
			self.piece_starts
				.retain(|staff, _| staves[*staff].page() != page_index);
			self.section_starts
				.retain(|staff, _| staves[*staff].page() != page_index);
			self.ensure_invariants();
		}

		let (_page, staves) = self.pages.remove(*page_index);

		self.shift_items(
			self.count_staves_before(page_index),
			None,
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
		if self.pages.iter().map(|p| p.1.len()).sum::<usize>() > 0 {
			self.shift_items(
				self.count_staves_before(page_index) + self.pages[*page_index].1.len(),
				None,
				staves.len() as isize,
			);
		}
		self.pages[*page_index].1.extend(staves);
		self.ensure_invariants();
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
		self.shift_items(self.count_staves_before(page_index) + index, None, 1);
		self.pages[*page_index].1.insert(index, staff);
		self.ensure_invariants();
		index
	}

	/** The `staff` parameter is relative to the page index */
	pub fn delete_staff(&mut self, page_index: PageIndex, staff: usize) {
		self.shift_items(self.count_staves_before(page_index) + staff + 1, None, -1);

		self.pages[*page_index].1.remove(staff);
		if !self.piece_starts.contains_key(&StaffIndex(0)) {
			self.piece_starts.insert(StaffIndex(0), "".into());
		}
		if !self.section_starts.contains_key(&StaffIndex(0)) {
			self.section_starts
				.insert(StaffIndex(0), SectionMeta::default());
		}
	}

	/// Move a single staff, update the y ordering
	pub fn move_staff(&mut self, page_index: PageIndex, staff: usize, dx: f64, dy: f64) -> usize {
		self.modify_staff(page_index, staff, |staff| {
			staff.start.0 += dx;
			staff.start.1 += dy;
			staff.end.0 += dx;
			staff.end.1 += dy;
		})
	}

	/// Modify a single staff, update the y ordering
	/// The staff *must* remain on the same page
	pub fn modify_staff(
		&mut self,
		page_index: PageIndex,
		staff: usize,
		modify: impl FnOnce(&mut Staff),
	) -> usize {
		let page: &mut Vec<Staff> = &mut self.pages[*page_index].1;

		let old_index = staff;
		let mut new_staff = page.remove(staff);
		let old_staff = new_staff.clone();
		modify(&mut new_staff);
		assert_eq!(old_staff.page(), new_staff.page());

		let new_index = page
			.iter()
			.enumerate()
			.find(|(_, staff2)| staff2.start.1 > new_staff.start.1)
			.map(|(index, _)| index)
			.unwrap_or_else(|| page.len());

		if new_index == old_index {
			page.insert(new_index, new_staff);
		} else {
			page.insert(new_index, new_staff);
			self.shift_items(
				self.count_staves_before(page_index) + old_index + 1,
				Some(self.count_staves_before(page_index) + new_index),
				-1,
			);
		}
		self.ensure_invariants();
		new_index
	}

	pub fn save(&self, file: std::path::PathBuf) -> anyhow::Result<()> {
		let song = SongMeta {
			n_pages: self.pages.len(),
			staves: self.get_staves(),
			piece_starts: self.piece_starts.clone(),
			section_starts: self.section_starts.clone(),
			song_uuid: self.song_uuid,
			version_uuid: uuid::Uuid::new_v4(),
			title: Some(&self.song_name)
				.filter(|name| !name.is_empty())
				.cloned(),
			composer: Some(&self.song_composer)
				.filter(|name| !name.is_empty())
				.cloned(),
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
