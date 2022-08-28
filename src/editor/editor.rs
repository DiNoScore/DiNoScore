use dinoscore::{collection::*, prelude::*, *};
use uuid::Uuid;

/// Business logic for the editor

struct FullStaff {
	staff: Staff,
	piece_start: Option<String>,
	section_start: Option<SectionMeta>,
}

/**
 * Representation of a [`collection::SongFile`] together with its
 * [`SongMeta`](collection::SongMeta) as required by the editor
 */
pub struct EditorSongFile {
	pages: TiVec<PageIndex, Arc<PageImage>>,
	staves: TiVec<StaffIndex, FullStaff>,

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
			pages: Default::default(),
			staves: Default::default(),
			song_uuid: Uuid::new_v4(),
			song_name: "".into(),
			song_composer: "".into(),
		}
	}

	pub fn get_staves(&self) -> TiVec<StaffIndex, Staff> {
		self.staves
			.iter()
			.map(|staff| staff.staff.clone())
			.collect()
	}

	pub fn get_page(&self, page: PageIndex) -> (&Arc<PageImage>, Vec<Staff>) {
		(
			&self.pages[page],
			self.staves
				.iter()
				.filter(|staff| staff.staff.page == page)
				.map(|staff| staff.staff.clone())
				.collect(),
		)
	}

	pub fn get_pages(&self) -> &TiSlice<PageIndex, Arc<PageImage>> {
		self.pages.as_slice()
	}

	pub fn piece_start(&self, staff: StaffIndex) -> Option<&String> {
		self.staves[staff].piece_start.as_ref()
	}

	pub fn section_start(&self, staff: StaffIndex) -> Option<SectionMeta> {
		self.staves[staff].section_start
	}

	pub fn piece_start_mut(&mut self, staff: StaffIndex) -> &mut Option<String> {
		&mut self.staves[staff].piece_start
	}

	pub fn section_start_mut(&mut self, staff: StaffIndex) -> &mut Option<SectionMeta> {
		&mut self.staves[staff].section_start
	}

	pub fn count_staves_before(&self, page: PageIndex) -> usize {
		self.staves
			.iter()
			.filter(|staff| staff.staff.page < page)
			.count()
	}

	pub fn count_sections_until(&self, staff: StaffIndex) -> usize {
		self.staves[..=staff]
			.iter()
			.filter(|staff| staff.section_start.is_some())
			.count()
	}

	/// Add a page to the end
	pub fn add_page(&mut self, page: PageImage) {
		self.pages.push(Arc::new(page));
	}

	pub fn remove_page(&mut self, page_index: PageIndex) {
		self.pages.remove(page_index);
		self.staves.retain(|staff| staff.staff.page != page_index);
		for staff in &mut self.staves {
			if staff.staff.page > page_index {
				staff.staff.page -= PageIndex(1);
			}
		}

		if let Some(staff) = self.staves.get_mut(StaffIndex(0)) {
			staff.piece_start.get_or_insert("".into());
			let _ = staff.section_start.insert(SectionMeta::default());
		}
	}

	/// Add staves to the end of a page. Staves must already be y-ordered
	pub fn add_staves(&mut self, page_index: PageIndex, staves: Vec<Staff>) {
		let index = self
			.staves
			.iter_enumerated()
			.find(|(_, staff)| staff.staff.page > page_index)
			.map(|(i, _)| i)
			.unwrap_or(StaffIndex(self.staves.len()));
		self.staves.splice(
			index..index,
			staves
				.into_iter()
				.inspect(|staff| assert_eq!(staff.page, page_index))
				.map(|staff| FullStaff {
					staff,
					piece_start: None,
					section_start: None,
				}),
		);

		if let Some(staff) = self.staves.get_mut(StaffIndex(0)) {
			staff.piece_start.get_or_insert("".into());
			let _ = staff.section_start.insert(SectionMeta::default());
		}
	}

	/// Insert single staff, maintain y ordering
	pub fn add_staff(&mut self, page_index: PageIndex, staff: Staff) -> usize {
		assert_eq!(page_index, staff.page);
		/* Find correct index to insert based on y coordinate */
		let (index_rel_page, index) = self
			.staves
			.iter_enumerated()
			.filter(|(_, staff)| staff.staff.page == page_index)
			.enumerate()
			.find(|(_, (_, staff2))| staff2.staff.start.1 > staff.start.1)
			.map(|(index_rel_page, (index, _))| (index_rel_page, index))
			.unwrap_or_else(|| {
				(
					self.staves
						.iter()
						.filter(|staff| staff.staff.page == page_index)
						.count(),
					StaffIndex(
						self.staves
							.iter()
							.filter(|staff| staff.staff.page <= page_index)
							.count(),
					),
				)
			});
		self.staves.insert(
			index,
			FullStaff {
				staff,
				piece_start: None,
				section_start: None,
			},
		);

		if let Some(staff) = self.staves.get_mut(StaffIndex(0)) {
			staff.piece_start.get_or_insert("".into());
			let _ = staff.section_start.insert(SectionMeta::default());
		}

		index_rel_page
	}

	/** The `staff` parameter is relative to the page index */
	pub fn delete_staff(&mut self, page_index: PageIndex, staff: usize) {
		let index = self
			.staves
			.iter_enumerated()
			.filter(|(_, staff)| staff.staff.page == page_index)
			.nth(staff)
			.map(|(index, _)| index)
			.expect("Tried to delete non-existent staff");
		self.staves.remove(index);

		if let Some(staff) = self.staves.get_mut(StaffIndex(0)) {
			staff.piece_start.get_or_insert("".into());
			let _ = staff.section_start.insert(SectionMeta::default());
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
		let index = self
			.staves
			.iter_enumerated()
			.filter(|(_, staff)| staff.staff.page == page_index)
			.nth(staff)
			.map(|(index, _)| index)
			.expect("Tried to modify non-existent staff");
		let mut staff = self.staves.remove(index);
		modify(&mut staff.staff);
		assert_eq!(page_index, staff.staff.page);

		/* Find correct index to insert based on y coordinate */
		let (index_rel_page, index) = self
			.staves
			.iter_enumerated()
			.filter(|(_, staff)| staff.staff.page == page_index)
			.enumerate()
			.find(|(_, (_, staff2))| staff2.staff.start.1 > staff.staff.start.1)
			.map(|(index_rel_page, (index, _))| (index_rel_page, index))
			.unwrap_or_else(|| {
				(
					self.staves
						.iter()
						.filter(|staff| staff.staff.page == page_index)
						.count(),
					StaffIndex(
						self.staves
							.iter()
							.filter(|staff| staff.staff.page <= page_index)
							.count(),
					),
				)
			});
		self.staves.insert(index, staff);

		if let Some(staff) = self.staves.get_mut(StaffIndex(0)) {
			staff.piece_start.get_or_insert("".into());
			let _ = staff.section_start.insert(SectionMeta::default());
		}

		index_rel_page
	}

	pub fn load(&mut self, song: SongMeta) {
		self.staves = song
			.staves
			.into_iter_enumerated()
			.map(|(index, staff)| FullStaff {
				staff,
				piece_start: song.piece_starts.get(&index).cloned(),
				section_start: song.section_starts.get(&index).cloned(),
			})
			.collect();
		self.song_name = song.title.unwrap_or_default();
		self.song_composer = song.composer.unwrap_or_default();
		self.song_uuid = song.song_uuid;
	}

	pub fn save(&self, file: std::path::PathBuf) -> anyhow::Result<()> {
		assert!(self.staves.len() > 0, "You need at least one staff to save");
		let song = SongMeta {
			n_pages: self.pages.len(),
			staves: self.get_staves(),
			piece_starts: self
				.staves
				.iter_enumerated()
				.filter_map(|(i, staff)| staff.piece_start.clone().map(|p| (i, p)))
				.collect(),
			section_starts: self
				.staves
				.iter_enumerated()
				.filter_map(|(i, staff)| staff.section_start.clone().map(|p| (i, p)))
				.collect(),
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
		let thumbnail = SongFile::generate_thumbnail(&song, self.pages.iter().map(Deref::deref))
			.expect("Failed to generate thumbnail");
		SongFile::save(
			file,
			song,
			self.pages.iter().map(Deref::deref),
			thumbnail,
			true, // TODO overwrite?!
		)?;
		Ok(())
	}
}
