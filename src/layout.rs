
use super::*;
use super::collection::*;

use noisy_float::prelude::*;

pub const EXPERIMENT_MODE: bool = true;

#[derive(Clone, Debug)]
pub struct StaffLayout {
	pub index: collection::StaffIndex,
	pub x: f64,
	pub y: f64,
	pub width: f64,
}

#[derive(Clone, Debug)]
pub struct PageLayout {
	/// Pages[Staves]
	pub pages: Vec<Vec<StaffLayout>>,
}

impl PageLayout {
	pub fn new(
		song: &collection::SongMeta,
		width: f64,
		height: f64,
		zoom: f64,
		column_count: usize,
		spacing: f64,
	) -> Self {
		if EXPERIMENT_MODE {
			return PageLayout::new_alternate(song, width, height, column_count);
		}
		let column_width = (width / column_count as f64) * zoom;
		/* 1. Segment the staves to fit onto columns */
		let column_starts = {
			let mut column_starts = Vec::<StaffIndex>::new();
			let mut y = 0.0;
			// let mut y_with_spacing = 0.0;
			for (index, staff) in song.staves.iter().enumerate() {
				let index = StaffIndex(index);

				let staff_height = column_width * staff.aspect_ratio();

				/*
				 * Start a new column for a new piece.
				 * If staff doesn't fit anymore, first try to squeeze it in at the cost of spacing
				 */
				if song.piece_starts.contains_key(&index)
					// || ((y_with_spacing + staff_height > height) && (y + staff_height <= height))
					|| (y + staff_height > height)
				{
					y = 0.0;
					// y_with_spacing = 0.0;
					column_starts.push(index);
				}
				y += staff_height;
				// y_with_spacing += staff_height + spacing;
			}
			/* Without this the last page will get swallowed */
			column_starts.push(song.staves.len().into());
			column_starts
		};

		/* 2. Calculate the exact position of each staff */
		let columns: Vec<Vec<StaffLayout>> = column_starts
			.windows(2)
			.map(|v| (v[0], v[1]))
			.map(|(chunk_start, chunk_end)| {
				let mut column = Vec::new();
				let staves: &[Staff] = &song.staves[chunk_start.into()..chunk_end.into()];
				if staves.len() == 1 {
					let staff = &staves[0];
					let staff_height = column_width * staff.aspect_ratio();
					let x;
					let y;
					let staff_width;

					if staff_height > height {
						staff_width = height / staff.aspect_ratio();
						x = (column_width - staff_width) / 2.0;
						y = 0.0;
					} else {
						staff_width = column_width;
						x = 0.0;
						y = (height - staff_height) / 2.0;
					}

					column.push(StaffLayout {
						index: chunk_start,
						x,
						y,
						width: staff_width,
					});
				} else {
					let excess_space = height
						- staves
							.iter()
							.map(|staff| column_width * staff.aspect_ratio())
							.sum::<f64>();
					let spacing = f64::min(spacing, excess_space / staves.len() as f64);
					let mut y = (excess_space - spacing * staves.len() as f64) / 2.0;
					for (index, staff) in staves.iter().enumerate() {
						column.push(StaffLayout {
							index: chunk_start + StaffIndex(index),
							x: 0.0,
							y,
							width: column_width,
						});
						y += column_width * staff.aspect_ratio() + spacing;
					}
				}
				column
			})
			.collect();

		/* 3. Merge the single columns to pages using iterator magic */
		let left_margin = (width - width * zoom) / 2.0;
		let pages = columns
			.chunks(column_count)
			.map(|chunk| {
				chunk
					.iter()
					.enumerate()
					.flat_map(|(i, c)| {
						c.iter().map(move |staff| StaffLayout {
							index: staff.index,
							x: staff.x + column_width * (i % column_count) as f64 + left_margin,
							y: staff.y,
							width: staff.width,
						})
					})
					.collect()
			})
			.collect();
		PageLayout { pages }
	}

	pub fn new_alternate(song: &collection::SongMeta, width: f64, height: f64, row_count: usize) -> Self {
		let row_height = height / row_count as f64;

		let column_starts = {
			let mut column_starts = Vec::<StaffIndex>::new();
			let mut page_length = 0;
			for index in 0..song.staves.len() {
				let index = StaffIndex(index);

				if song.piece_starts.contains_key(&index) || page_length >= row_count {
					column_starts.push(index);
					page_length = 0;
				}
				page_length += 1;
			}
			column_starts.push(song.staves.len().into());
			column_starts
		};

		let pages = column_starts
			.windows(2)
			.map(|v| (v[0], v[1]))
			.map(|(chunk_start, chunk_end)| {
				let staves: &[Staff] = &song.staves[chunk_start.into()..chunk_end.into()];
				let max_width: f64 = staves
					.iter()
					.map(|staff| r64(row_height / staff.aspect_ratio()))
					.min() /* min is correct here */
					.expect("Page cannot be empty")
					.into();
				let max_width = max_width.min(width);

				staves
					.iter()
					.enumerate()
					.map(|(in_page_index, staff)| {
						let mut staff_width = row_height / staff.aspect_ratio();
						let mut staff_height = row_height;

						if staff_width > max_width {
							staff_width = max_width;
							staff_height *= staff_width / max_width;
						}

						StaffLayout {
							index: StaffIndex(in_page_index) + chunk_start,
							x: (width - staff_width) / 2.0,
							y: in_page_index as f64 * row_height
								+ (row_height - staff_height) / 2.0,
							width: staff_width,
						}
					})
					.collect::<Vec<_>>()
			})
			.collect::<Vec<_>>();

		PageLayout { pages }
	}

	/** Get the index of the staff at the center of the page. */
	pub fn get_center_staff(&self, page: PageIndex) -> StaffIndex {
		StaffIndex(
			self.pages[0..*page].iter().map(Vec::len).sum::<usize>() + self.pages[*page].len() / 2,
		)
	}

	pub fn get_staves_of_page<'a>(&'a self, page: PageIndex) -> impl Iterator<Item = StaffIndex> + 'a {
		self.pages[*page].iter().map(|page| page.index)
	}

	pub fn get_page_of_staff(&self, staff: StaffIndex) -> PageIndex {
		let mut sum = 0;
		for (i, page) in self.pages.iter().enumerate() {
			sum += page.len();
			if sum > staff.into() {
				return i.into();
			}
		}
		unreachable!()
	}
}
